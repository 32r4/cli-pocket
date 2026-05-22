#[must_use]
pub fn next_delay(cur_ms: u64, max_ms: u64, mul_x10: u32) -> u64 {
    let scaled = (u128::from(cur_ms) * u128::from(mul_x10)) / 10;
    u64::try_from(scaled)
        .unwrap_or(u64::MAX)
        .min(max_ms)
        .max(50)
}

#[must_use]
pub fn jitter(base_ms: u64, rng_byte: u8) -> u64 {
    let offset = i64::from(rng_byte) - 128;
    let delta = (i128::from(base_ms) * i128::from(offset)) / 512;
    let jittered = i128::from(base_ms) + delta;
    if jittered <= 0 {
        1
    } else {
        u64::try_from(jittered).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_at_max() {
        assert_eq!(next_delay(1000, 5000, 30), 3000);
        assert_eq!(next_delay(3000, 5000, 30), 5000);
        assert_eq!(next_delay(10_000, 5000, 30), 5000);
    }

    #[test]
    fn jitter_within_bounds() {
        let low = jitter(1000, 0);
        assert!((749..=1001).contains(&low));
        let high = jitter(1000, 255);
        assert!((999..=1251).contains(&high));
    }
}
