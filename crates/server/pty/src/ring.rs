use crate::parser::AnchorTracker;
use cli_pocket_proto::{AnchorState, DeltaSlice, Snapshot, StreamSeq};
use serde_bytes::ByteBuf;
use std::collections::VecDeque;

const DEFAULT_CAPACITY_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_ANCHOR_INTERVAL: usize = 64 * 1024;
const MAX_CAPACITY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RingError {
    #[error("capacity {0} exceeds the per-terminal limit {MAX_CAPACITY_BYTES}")]
    CapacityTooLarge(usize),
}

#[derive(Debug, Clone)]
struct Anchor {
    byte_offset: u64,
    state: AnchorState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySlice {
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub bytes: ByteBuf,
}

pub struct ScrollbackRing {
    bytes: VecDeque<u8>,
    anchors: VecDeque<Anchor>,
    head_seq: StreamSeq,
    tail_offset: u64,
    tracker: AnchorTracker,
    bytes_since_anchor: usize,
    cap: usize,
    anchor_interval: usize,
    cols: u16,
    rows: u16,
}

impl ScrollbackRing {
    pub fn new(cols: u16, rows: u16, capacity: Option<usize>) -> Result<Self, RingError> {
        let cap = capacity.unwrap_or(DEFAULT_CAPACITY_BYTES);
        if cap > MAX_CAPACITY_BYTES {
            return Err(RingError::CapacityTooLarge(cap));
        }

        let mut ring = Self {
            bytes: VecDeque::with_capacity(cap.min(DEFAULT_ANCHOR_INTERVAL)),
            anchors: VecDeque::new(),
            head_seq: StreamSeq(0),
            tail_offset: 0,
            tracker: AnchorTracker::new(),
            bytes_since_anchor: usize::MAX,
            cap,
            anchor_interval: DEFAULT_ANCHOR_INTERVAL.min(cap.max(1)),
            cols,
            rows,
        };
        ring.maybe_place_anchor();
        Ok(ring)
    }

    #[must_use]
    pub fn head_seq(&self) -> StreamSeq {
        self.head_seq
    }

    #[must_use]
    pub fn tail_seq(&self) -> StreamSeq {
        StreamSeq(self.tail_offset)
    }

    #[must_use]
    pub fn dims(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        self.tracker.snapshot_state().cursor
    }

    pub fn set_dims(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    pub fn push(&mut self, chunk: &[u8]) {
        for &byte in chunk {
            self.tracker.advance(&[byte]);
            self.bytes.push_back(byte);
            self.head_seq.0 = self.head_seq.0.saturating_add(1);
            self.bytes_since_anchor = self.bytes_since_anchor.saturating_add(1);
            self.maybe_place_anchor();
            self.maybe_evict();
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let anchor = self.anchors.front().cloned().unwrap_or_else(|| Anchor {
            byte_offset: self.tail_offset,
            state: self.tracker.snapshot_state(),
        });
        let start_rel = offset_to_relative(anchor.byte_offset, self.tail_offset);
        let bytes: Vec<u8> = self.bytes.iter().skip(start_rel).copied().collect();

        Snapshot {
            cols: self.cols,
            rows: self.rows,
            anchor_state: anchor.state,
            bytes: ByteBuf::from(bytes),
            head_seq: self.head_seq,
        }
    }

    #[must_use]
    pub fn since(&self, seq: StreamSeq) -> Option<DeltaSlice> {
        if seq.0 < self.tail_offset {
            return None;
        }

        if seq.0 > self.head_seq.0 {
            return None;
        }

        let start_rel = offset_to_relative(seq.0, self.tail_offset);
        let bytes: Vec<u8> = self.bytes.iter().skip(start_rel).copied().collect();
        Some(DeltaSlice {
            bytes: ByteBuf::from(bytes),
            head_seq: self.head_seq,
        })
    }

    #[must_use]
    pub fn history_page(&self, before: Option<StreamSeq>, max_bytes: usize) -> HistorySlice {
        let end_offset = before
            .map_or(self.head_seq.0, |seq| seq.0)
            .clamp(self.tail_offset, self.head_seq.0);
        let available = end_offset.saturating_sub(self.tail_offset);
        let page_len = available.min(u64::try_from(max_bytes).unwrap_or(u64::MAX));
        let start_offset = end_offset.saturating_sub(page_len);
        let start_rel = offset_to_relative(start_offset, self.tail_offset);
        let byte_len = usize::try_from(page_len).unwrap_or(usize::MAX);
        let bytes: Vec<u8> = self
            .bytes
            .iter()
            .skip(start_rel)
            .take(byte_len)
            .copied()
            .collect();

        HistorySlice {
            start_seq: StreamSeq(start_offset),
            end_seq: StreamSeq(end_offset),
            bytes: ByteBuf::from(bytes),
        }
    }

    fn maybe_place_anchor(&mut self) {
        if !self.anchors.is_empty() && self.bytes_since_anchor < self.anchor_interval {
            return;
        }

        if !self.anchors.is_empty() && !self.tracker.is_at_safe_split() {
            return;
        }

        self.anchors.push_back(Anchor {
            byte_offset: self.head_seq.0,
            state: self.tracker.snapshot_state(),
        });
        self.bytes_since_anchor = 0;
    }

    fn maybe_evict(&mut self) {
        while self.bytes.len() > self.cap {
            let Some(eviction_target) = self.eviction_target() else {
                break;
            };
            let to_drop = offset_to_relative(eviction_target, self.tail_offset);

            if to_drop == 0 {
                break;
            }

            self.bytes.drain(..to_drop);
            self.tail_offset = self.tail_offset.saturating_add(to_drop as u64);
            self.drop_stale_anchors();
        }
    }

    fn eviction_target(&self) -> Option<u64> {
        self.anchors
            .get(1)
            .map(|anchor| anchor.byte_offset)
            .filter(|&offset| offset > self.tail_offset)
    }

    fn drop_stale_anchors(&mut self) {
        while self
            .anchors
            .get(1)
            .is_some_and(|anchor| anchor.byte_offset <= self.tail_offset)
        {
            self.anchors.pop_front();
        }
    }
}

fn offset_to_relative(offset: u64, tail_offset: u64) -> usize {
    usize::try_from(offset.saturating_sub(tail_offset)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn fresh_ring_has_anchor_at_offset_zero() {
        let ring = ScrollbackRing::new(80, 24, None).unwrap();

        assert_eq!(ring.head_seq(), StreamSeq(0));
        assert_eq!(ring.tail_seq(), StreamSeq(0));
        let snapshot = ring.snapshot();
        assert_eq!(snapshot.cols, 80);
        assert_eq!(snapshot.rows, 24);
        assert!(snapshot.bytes.is_empty());
    }

    #[test]
    fn push_advances_head_seq_and_snapshot_bytes() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"hello");

        assert_eq!(ring.head_seq(), StreamSeq(5));
        assert_eq!(&ring.snapshot().bytes[..], b"hello");
    }

    #[test]
    fn since_returns_delta_within_window() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"abc");
        ring.push(b"def");

        let delta = ring.since(StreamSeq(3)).unwrap();
        assert_eq!(&delta.bytes[..], b"def");
        assert_eq!(delta.head_seq, StreamSeq(6));

        let empty_delta = ring.since(StreamSeq(6)).unwrap();
        assert!(empty_delta.bytes.is_empty());
        assert_eq!(empty_delta.head_seq, StreamSeq(6));
    }

    #[test]
    fn since_rejects_future_seq() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"abc");

        assert!(ring.since(StreamSeq(4)).is_none());
    }

    #[test]
    fn since_returns_none_below_tail_after_eviction() {
        let mut ring = ScrollbackRing::new(80, 24, Some(64)).unwrap();
        let chunk = vec![b'A'; 8 * 1024];
        for _ in 0..16 {
            ring.push(&chunk);
        }

        let tail = ring.tail_seq();

        assert!(tail.0 > 0);
        assert!(ring.since(StreamSeq(tail.0 - 1)).is_none());
        assert!(ring.since(ring.head_seq()).unwrap().bytes.is_empty());
        assert_eq!(
            ring.anchors.front().map(|anchor| anchor.byte_offset),
            Some(tail.0)
        );
        assert!(ring.snapshot().bytes.len() <= 64);
    }

    #[test]
    fn history_page_returns_latest_window_when_before_is_absent() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"abcdefghij");

        let page = ring.history_page(None, 4);

        assert_eq!(page.start_seq, StreamSeq(6));
        assert_eq!(page.end_seq, StreamSeq(10));
        assert_eq!(&page.bytes[..], b"ghij");
    }

    #[test]
    fn history_page_clamps_before_to_retained_window() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"abcdefghij");

        let page = ring.history_page(Some(StreamSeq(4)), 8);

        assert_eq!(page.start_seq, StreamSeq(0));
        assert_eq!(page.end_seq, StreamSeq(4));
        assert_eq!(&page.bytes[..], b"abcd");
    }

    #[test]
    fn history_page_returns_empty_slice_when_before_is_at_tail() {
        let mut ring = ScrollbackRing::new(80, 24, None).unwrap();

        ring.push(b"abcdefghij");

        let page = ring.history_page(Some(StreamSeq(0)), 8);

        assert_eq!(page.start_seq, StreamSeq(0));
        assert_eq!(page.end_seq, StreamSeq(0));
        assert!(page.bytes.is_empty());
    }

    #[test]
    fn long_unterminated_osc_keeps_original_safe_anchor() {
        let mut ring = ScrollbackRing::new(80, 24, Some(64)).unwrap();

        ring.push(b"\x1b]");
        ring.push(&vec![b'x'; 512]);

        let snapshot = ring.snapshot();

        assert_eq!(ring.tail_seq(), StreamSeq(0));
        assert_eq!(ring.anchors.len(), 1);
        assert!(snapshot.bytes.starts_with(b"\x1b]"));
        assert!(snapshot.bytes.len() > 64);
        assert_eq!(
            ring.anchors.front().map(|anchor| anchor.byte_offset),
            Some(ring.tail_seq().0)
        );
    }

    proptest! {
        #[test]
        fn random_pushes_preserve_window_invariants(
            chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..256), 0..40),
        ) {
            let mut ring = ScrollbackRing::new(80, 24, Some(8 * 1024)).unwrap();
            for chunk in chunks {
                ring.push(&chunk);
                let snapshot = ring.snapshot();

                prop_assert!(ring.head_seq().0 >= ring.tail_seq().0);
                prop_assert!(snapshot.bytes.len() <= 8 * 1024 || ring.anchors.len() == 1);
                prop_assert_eq!(
                    snapshot.bytes.len() as u64,
                    ring.head_seq().0.saturating_sub(ring.tail_seq().0)
                );
                prop_assert_eq!(
                    ring.anchors.front().map(|anchor| anchor.byte_offset),
                    Some(ring.tail_seq().0)
                );
            }
        }
    }
}
