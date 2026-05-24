use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;
use std::sync::Arc;

#[derive(Clone)]
pub struct PairingCodes {
    inner: Arc<Mutex<PairingCodeState>>,
    ttl: Duration,
}

#[derive(Debug)]
struct PairingCodeState {
    code: String,
    generated_at: Instant,
}

impl PairingCodes {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PairingCodeState {
                code: generate_pair_code(),
                generated_at: Instant::now(),
            })),
            ttl,
        }
    }

    pub fn current_code(&self) -> String {
        self.inner.lock().code.clone()
    }

    pub fn rotate(&self) -> String {
        let mut state = self.inner.lock();
        state.code = generate_pair_code_except(&state.code);
        state.generated_at = Instant::now();
        state.code.clone()
    }

    pub fn rotate_if_expired(&self) -> Option<String> {
        let mut state = self.inner.lock();
        if state.generated_at.elapsed() < self.ttl {
            return None;
        }

        state.code = generate_pair_code_except(&state.code);
        state.generated_at = Instant::now();
        Some(state.code.clone())
    }

    pub fn match_current(&self, code: &str) -> bool {
        let state = self.inner.lock();
        state.code == code
    }

    pub fn consume_current(&self, code: &str) -> Option<String> {
        let mut state = self.inner.lock();
        if state.code != code {
            return None;
        }

        state.code = generate_pair_code_except(&state.code);
        state.generated_at = Instant::now();
        Some(state.code.clone())
    }
}

fn generate_pair_code() -> String {
    let mut rng = rand::thread_rng();
    let n: u32 = rng.gen_range(0..1_000_000);
    format!("{n:06}")
}

fn generate_pair_code_except(previous: &str) -> String {
    for _ in 0..16 {
        let next = generate_pair_code();
        if next != previous {
            return next;
        }
    }

    let fallback = previous.parse::<u32>().map_or(0, |n| (n + 1) % 1_000_000);
    format!("{fallback:06}")
}
