use cli_pocket_relay_core::caps::Caps;

#[test]
fn host_limit_enforced() {
    let c = Caps::new(2, 4, 1024, 1024);
    let _h1 = c.try_add_host().unwrap();
    let _h2 = c.try_add_host().unwrap();
    assert!(c.try_add_host().is_err());
    drop(_h1);
    assert!(c.try_add_host().is_ok());
}

#[test]
fn rate_burst_then_throttle() {
    let c = Caps::new(1, 1, 1000, 4096);
    let pair = c.try_add_pair().unwrap();
    // Exhaust the 1000-byte bucket: 10 × 100 = 1000.
    for _ in 0..10 {
        assert!(pair.try_consume_rate(100).is_ok());
    }
    // Bucket is empty — any further consume must fail.
    assert!(pair.try_consume_rate(1).is_err());
    // One tick refills max_bytes_per_sec / 10 = 100 bytes.
    pair.refill_one_tick();
    // 90 bytes fits into the 100-byte refill; 100 would also work.
    assert!(pair.try_consume_rate(90).is_ok());
}
