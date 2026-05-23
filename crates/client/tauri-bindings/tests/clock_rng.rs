use cli_pocket_client_core::{Clock, Rng};
use cli_pocket_tauri_bindings::{OsRandom, TokioClock};

#[tokio::test]
async fn clock_now_is_process_local_elapsed_ms() {
    let clock = TokioClock;

    let before = clock.now_ms();
    clock.sleep_ms(5).await;
    let after = clock.now_ms();

    assert!(after >= before, "{after} >= {before}");
    assert!(after < 60_000, "{after} should be process-local elapsed ms");
}

#[test]
fn rng_accepts_requested_buffer_without_touching_guards() {
    let rng = OsRandom;
    let mut bytes = [0xaa_u8; 34];

    rng.fill(&mut bytes[1..33]);

    assert_eq!(bytes[0], 0xaa);
    assert_eq!(bytes[33], 0xaa);
}
