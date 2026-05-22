use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Shared capacity counters for the relay.
#[derive(Clone)]
pub struct Caps {
    inner: Arc<CapsInner>,
}

struct CapsInner {
    max_hosts: usize,
    max_pairs: usize,
    max_bytes_per_sec: u64,
    max_queued_bytes: usize,
    hosts: AtomicUsize,
    pairs: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapsSnapshot {
    pub hosts: usize,
    pub pairs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairCapsSnapshot {
    pub rate_remaining: u64,
    pub queued_bytes: usize,
}

impl Caps {
    #[must_use]
    pub fn new(
        max_hosts: usize,
        max_pairs: usize,
        max_bytes_per_sec: u64,
        max_queued_bytes: usize,
    ) -> Self {
        Self {
            inner: Arc::new(CapsInner {
                max_hosts,
                max_pairs,
                max_bytes_per_sec,
                max_queued_bytes,
                hosts: AtomicUsize::new(0),
                pairs: AtomicUsize::new(0),
            }),
        }
    }

    pub fn try_add_host(&self) -> crate::RelayResult<HostTicket> {
        increment_bounded_usize(&self.inner.hosts, self.inner.max_hosts, "max_hosts")?;
        Ok(HostTicket {
            inner: Arc::clone(&self.inner),
        })
    }

    pub fn try_add_pair(&self) -> crate::RelayResult<PairTicket> {
        increment_bounded_usize(&self.inner.pairs, self.inner.max_pairs, "max_pairs")?;
        Ok(PairTicket {
            inner: Arc::clone(&self.inner),
            rate_bucket: AtomicU64::new(self.inner.max_bytes_per_sec),
            rate_remainder_tenths: AtomicU64::new(0),
            queued_bytes: AtomicUsize::new(0),
        })
    }

    #[must_use]
    pub fn max_queued_bytes(&self) -> usize {
        self.inner.max_queued_bytes
    }

    #[must_use]
    pub fn snapshot(&self) -> CapsSnapshot {
        CapsSnapshot {
            hosts: self.inner.hosts.load(Ordering::Acquire),
            pairs: self.inner.pairs.load(Ordering::Acquire),
        }
    }

    #[must_use]
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub struct HostTicket {
    inner: Arc<CapsInner>,
}

impl Drop for HostTicket {
    fn drop(&mut self) {
        self.inner
            .hosts
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(1)
            })
            .ok();
    }
}

pub struct PairTicket {
    inner: Arc<CapsInner>,
    rate_bucket: AtomicU64,
    rate_remainder_tenths: AtomicU64,
    queued_bytes: AtomicUsize,
}

impl PairTicket {
    pub fn try_consume_rate(&self, bytes: u64) -> crate::RelayResult<()> {
        let mut current = self.rate_bucket.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_sub(bytes) else {
                return Err(crate::RelayError::OverCapacity("max_bytes_per_sec"));
            };

            match self.rate_bucket.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    pub fn refill_one_tick(&self) {
        let tick_tenths = self.inner.max_bytes_per_sec;
        let mut current_remainder = self.rate_remainder_tenths.load(Ordering::Acquire);
        let tick_bytes = loop {
            let total_tenths = current_remainder.saturating_add(tick_tenths);
            let whole_bytes = total_tenths / 10;
            let next_remainder = total_tenths % 10;

            match self.rate_remainder_tenths.compare_exchange(
                current_remainder,
                next_remainder,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break whole_bytes,
                Err(observed) => current_remainder = observed,
            }
        };
        if tick_bytes == 0 {
            return;
        }

        let mut current = self.rate_bucket.load(Ordering::Acquire);
        loop {
            let next = current
                .saturating_add(tick_bytes)
                .min(self.inner.max_bytes_per_sec);
            if next == current {
                return;
            }

            match self.rate_bucket.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    pub fn try_add_queued_bytes(&self, bytes: usize) -> crate::RelayResult<()> {
        let mut current = self.queued_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return Err(crate::RelayError::OverCapacity("max_queued_bytes"));
            };
            if next > self.inner.max_queued_bytes {
                return Err(crate::RelayError::OverCapacity("max_queued_bytes"));
            }

            match self.queued_bytes.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    pub fn remove_queued_bytes(&self, bytes: usize) {
        let mut current = self.queued_bytes.load(Ordering::Acquire);
        loop {
            let next = current.saturating_sub(bytes);
            match self.queued_bytes.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> PairCapsSnapshot {
        PairCapsSnapshot {
            rate_remaining: self.rate_bucket.load(Ordering::Acquire),
            queued_bytes: self.queued_bytes.load(Ordering::Acquire),
        }
    }
}

impl Drop for PairTicket {
    fn drop(&mut self) {
        self.inner
            .pairs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(1)
            })
            .ok();
    }
}

fn increment_bounded_usize(
    counter: &AtomicUsize,
    limit: usize,
    label: &'static str,
) -> crate::RelayResult<()> {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        if current >= limit {
            return Err(crate::RelayError::OverCapacity(label));
        }
        let next = current + 1;

        match counter.compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}
