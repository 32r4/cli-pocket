use cli_pocket_relay_core::caps::{Caps, CapsSnapshot};

#[test]
fn host_increment_decrement() {
    let c = Caps::new(2, 4, 1024, 1024);

    let h1 = c.try_add_host().unwrap();
    let _h2 = c.try_add_host().unwrap();
    assert!(c.try_add_host().is_err());

    drop(h1);

    assert!(c.try_add_host().is_ok());
}

#[test]
fn host_ticket_releases_capacity_on_drop() {
    let c = Caps::new(1, 4, 1024, 1024);
    let ticket = c.try_add_host().unwrap();

    assert!(c.try_add_host().is_err());

    drop(ticket);

    assert!(c.try_add_host().is_ok());
}

#[test]
fn live_host_ticket_blocks_admission_through_cloned_handle() {
    let c = Caps::new(1, 4, 1024, 1024);
    let cloned = c.clone_handle();
    let ticket = c.try_add_host().unwrap();

    assert!(cloned.try_add_host().is_err());
    assert_eq!(c.snapshot().hosts, 1);

    drop(ticket);

    assert!(cloned.try_add_host().is_ok());
}

#[test]
fn pair_independent_of_hosts() {
    let c = Caps::new(1, 3, 1024, 1024);
    let _host = c.try_add_host().unwrap();

    let _p1 = c.try_add_pair().unwrap();
    let _p2 = c.try_add_pair().unwrap();
    let _p3 = c.try_add_pair().unwrap();
    assert!(c.try_add_pair().is_err());
}

#[test]
fn pair_ticket_releases_capacity_on_drop() {
    let c = Caps::new(1, 1, 1024, 1024);
    let ticket = c.try_add_pair().unwrap();

    assert!(c.try_add_pair().is_err());

    drop(ticket);

    assert!(c.try_add_pair().is_ok());
}

#[test]
fn rate_bucket_refills_one_hundred_ms_ticks() {
    let c = Caps::new(1, 1, 100, 4096);
    let pair = c.try_add_pair().unwrap();

    assert!(pair.try_consume_rate(100).is_ok());
    assert!(pair.try_consume_rate(1).is_err());

    pair.refill_one_tick();

    assert_eq!(pair.snapshot().rate_remaining, 10);
    assert!(pair.try_consume_rate(10).is_ok());
    assert!(pair.try_consume_rate(1).is_err());
}

#[test]
fn rate_bucket_refill_caps_at_one_second_capacity() {
    let c = Caps::new(1, 1, 100, 4096);
    let pair = c.try_add_pair().unwrap();

    for _ in 0..20 {
        pair.refill_one_tick();
    }

    assert_eq!(pair.snapshot().rate_remaining, 100);
}

#[test]
fn low_rate_refills_do_not_exceed_configured_rate() {
    let c = Caps::new(1, 1, 1, 4096);
    let pair = c.try_add_pair().unwrap();

    assert!(pair.try_consume_rate(1).is_ok());

    for _ in 0..9 {
        pair.refill_one_tick();
        assert_eq!(pair.snapshot().rate_remaining, 0);
    }

    pair.refill_one_tick();

    assert_eq!(pair.snapshot().rate_remaining, 1);
    assert!(pair.try_consume_rate(1).is_ok());
    assert!(pair.try_consume_rate(1).is_err());
}

#[test]
fn rate_buckets_are_independent_per_pair() {
    let c = Caps::new(1, 2, 100, 4096);
    let pair_a = c.try_add_pair().unwrap();
    let pair_b = c.try_add_pair().unwrap();

    assert!(pair_a.try_consume_rate(100).is_ok());
    assert!(pair_a.try_consume_rate(1).is_err());

    assert!(pair_b.try_consume_rate(100).is_ok());
    assert!(pair_b.try_consume_rate(1).is_err());
}

#[test]
fn low_rate_refill_accumulates_independently_per_pair() {
    let c = Caps::new(1, 2, 1, 4096);
    let pair_a = c.try_add_pair().unwrap();
    let pair_b = c.try_add_pair().unwrap();

    assert!(pair_a.try_consume_rate(1).is_ok());
    assert!(pair_b.try_consume_rate(1).is_ok());

    for _ in 0..10 {
        pair_a.refill_one_tick();
    }

    assert!(pair_a.try_consume_rate(1).is_ok());
    assert!(pair_b.try_consume_rate(1).is_err());

    for _ in 0..10 {
        pair_b.refill_one_tick();
    }

    assert!(pair_b.try_consume_rate(1).is_ok());
}

#[test]
fn queued_bytes_are_bounded_and_released() {
    let c = Caps::new(1, 1, 1024, 8);
    let pair = c.try_add_pair().unwrap();

    assert!(pair.try_add_queued_bytes(5).is_ok());
    assert!(pair.try_add_queued_bytes(3).is_ok());
    assert!(pair.try_add_queued_bytes(1).is_err());

    pair.remove_queued_bytes(4);

    assert!(pair.try_add_queued_bytes(4).is_ok());
    assert!(pair.try_add_queued_bytes(1).is_err());
}

#[test]
fn queued_bytes_are_independent_per_pair() {
    let c = Caps::new(1, 2, 1024, 8);
    let pair_a = c.try_add_pair().unwrap();
    let pair_b = c.try_add_pair().unwrap();

    assert!(pair_a.try_add_queued_bytes(8).is_ok());
    assert!(pair_a.try_add_queued_bytes(1).is_err());

    assert!(pair_b.try_add_queued_bytes(8).is_ok());
    assert!(pair_b.try_add_queued_bytes(1).is_err());
}

#[test]
fn snapshot_captures_state() {
    let c = Caps::new(2, 4, 1024, 1024);
    let _host = c.try_add_host().unwrap();
    let pair = c.try_add_pair().unwrap();
    pair.try_consume_rate(24).unwrap();
    pair.try_add_queued_bytes(64).unwrap();

    let s: CapsSnapshot = c.snapshot();
    let pair_s = pair.snapshot();

    assert_eq!(s.hosts, 1);
    assert_eq!(s.pairs, 1);
    assert_eq!(pair_s.rate_remaining, 1000);
    assert_eq!(pair_s.queued_bytes, 64);
}
