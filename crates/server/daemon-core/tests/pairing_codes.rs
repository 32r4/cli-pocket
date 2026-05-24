use std::time::Duration;

use cli_pocket_daemon_core::pairing::PairingCodes;

#[tokio::test(flavor = "current_thread")]
async fn starts_with_six_digit_code() {
    let codes = PairingCodes::new(Duration::from_secs(120));

    assert!(codes.current_code().chars().all(|ch| ch.is_ascii_digit()));
    assert_eq!(codes.current_code().len(), 6);
}

#[tokio::test(flavor = "current_thread")]
async fn rotate_changes_current_code() {
    let codes = PairingCodes::new(Duration::from_secs(120));
    let before = codes.current_code();

    let after = codes.rotate();

    assert_ne!(after, before);
    assert_eq!(codes.current_code(), after);
}

#[tokio::test(flavor = "current_thread")]
async fn current_match_preserves_code_until_consumed() {
    let codes = PairingCodes::new(Duration::from_secs(120));
    let before = codes.current_code();

    assert!(codes.match_current(&before));

    assert_eq!(codes.current_code(), before);
    let after = codes
        .consume_current(&before)
        .expect("consume should rotate");
    assert_ne!(codes.current_code(), before);
    assert_eq!(codes.current_code(), after);
    assert!(!codes.match_current(&before));
}

#[tokio::test(flavor = "current_thread")]
async fn ttl_expiry_rotates_code() {
    let codes = PairingCodes::new(Duration::ZERO);
    let before = codes.current_code();

    codes.rotate_if_expired();

    assert_ne!(codes.current_code(), before);
}
