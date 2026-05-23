//! Integration test placeholder.
//!
//! The end-to-end roundtrip (fake host + fake client through `RelayServer`,
//! one `RelayData::Forward` echo, then `PairClose`) lives behind a `#[ignore]`
//! guard until Task E7 lands the `RelayServer` facade and the in-process
//! `tokio-tungstenite` wiring. Once that arrives the stub below is unlocked
//! and fleshed out with the real assertions.

#[test]
#[ignore = "integration test enabled in Task E7"]
fn pair_offer_accept_roundtrip() {
    // placeholder
}
