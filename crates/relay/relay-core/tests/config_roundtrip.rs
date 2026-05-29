use cli_pocket_relay_core::RelayConfig;

#[test]
fn defaults_roundtrip() {
    let c = RelayConfig::default();
    let t = toml::to_string(&c).unwrap();
    let p: RelayConfig = toml::from_str(&t).unwrap();
    assert_eq!(p.listen.port, c.listen.port);
    assert_eq!(p.caps.max_servers, c.caps.max_servers);
}

#[test]
fn user_minimal_parses() {
    let t = r#"
[listen]
addr = "0.0.0.0"
port = 9000

[caps]
max_servers = 100
max_pairs = 1000
max_bytes_per_sec = 1048576
max_queued_bytes = 4194304

[guillotine]
idle_seconds = 30
"#;
    let p: RelayConfig = toml::from_str(t).unwrap();
    assert_eq!(p.listen.port, 9000);
    assert_eq!(p.caps.max_servers, 100);
    assert_eq!(p.guillotine.idle_seconds, 30);
}
