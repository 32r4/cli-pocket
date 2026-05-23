use cli_pocket_daemon_core::resume::ResumeTokenSecret;
use cli_pocket_proto::{ClientId, TerminalId};
use uuid::Uuid;

fn test_cid() -> ClientId {
    ClientId(Uuid::from_bytes([1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]))
}

fn test_tid() -> TerminalId {
    TerminalId(Uuid::from_bytes([16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]))
}

#[test]
fn mint_and_verify_roundtrip() {
    let secret = ResumeTokenSecret::from_bytes([42u8; 32]);
    let cid = test_cid();
    let tid = test_tid();
    let token = secret.mint(&cid, &tid, 1_700_000_000_000, 60_000);
    let verified = secret.verify(&token, 1_700_000_030_000).unwrap();
    assert_eq!(verified.client_id, cid);
    assert_eq!(verified.terminal_id, tid);
}

#[test]
fn expired_rejected() {
    let secret = ResumeTokenSecret::from_bytes([42u8; 32]);
    let token = secret.mint(&test_cid(), &test_tid(), 1_000, 100);
    assert!(secret.verify(&token, 2_000).is_err());
}

#[test]
fn forged_tag_rejected() {
    let secret = ResumeTokenSecret::from_bytes([1u8; 32]);
    let mut token = secret.mint(&test_cid(), &test_tid(), 0, 10_000);
    let last = token.bytes.len() - 1;
    token.bytes[last] ^= 0x01;
    assert!(secret.verify(&token, 1_000).is_err());
}

#[test]
fn different_key_rejected() {
    let s1 = ResumeTokenSecret::from_bytes([1u8; 32]);
    let s2 = ResumeTokenSecret::from_bytes([2u8; 32]);
    let token = s1.mint(&test_cid(), &test_tid(), 0, 10_000);
    assert!(s2.verify(&token, 1_000).is_err());
}
