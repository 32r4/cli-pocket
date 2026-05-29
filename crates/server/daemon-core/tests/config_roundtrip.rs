use cli_pocket_daemon_core::DaemonConfig;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[test]
fn default_then_serialize_then_deserialize() {
    let cfg = DaemonConfig::default();
    let toml_text = toml::to_string(&cfg).unwrap();
    let parsed: DaemonConfig = toml::from_str(&toml_text).unwrap();

    assert_eq!(parsed.listen.addr, cfg.listen.addr);
    assert_eq!(parsed.listen.port, cfg.listen.port);
    assert_eq!(parsed.security.identity_path, cfg.security.identity_path);
    assert_eq!(parsed.relay.base_url, cfg.relay.base_url);
    assert_eq!(parsed.relay.psk_hex, cfg.relay.psk_hex);
    assert_eq!(parsed.app.base_url, cfg.app.base_url);
}

#[test]
fn parses_minimal_user_config() {
    let toml_text = r#"
[listen]
addr = "0.0.0.0"
port = 8443

[relay]
base_url = "wss://relay.example.com"
psk_hex = "deadbeef"

[app]
base_url = "https://cli-pocket.example"
"#;

    let cfg: DaemonConfig = toml::from_str(toml_text).unwrap();

    assert_eq!(cfg.listen.port, 8443);
    assert_eq!(cfg.app.base_url, "https://cli-pocket.example");
    let relay = cfg.relay;
    assert_eq!(relay.base_url, "wss://relay.example.com");
    assert_eq!(relay.psk_hex, "deadbeef");
}

#[test]
fn app_base_url_roundtrips() {
    let toml_text = r#"
[relay]
base_url = "wss://relay.example.com"
psk_hex = "deadbeef"

[app]
base_url = "https://cli-pocket.32r4.asia/"
"#;

    let cfg: DaemonConfig = toml::from_str(toml_text).unwrap();

    assert_eq!(cfg.app.base_url, "https://cli-pocket.32r4.asia/");

    let roundtrip = toml::to_string(&cfg).unwrap();
    let reparsed: DaemonConfig = toml::from_str(&roundtrip).unwrap();
    assert_eq!(reparsed.app.base_url, "https://cli-pocket.32r4.asia/");
}

#[test]
fn missing_relay_is_rejected() {
    let toml_text = r#"
[listen]
addr = "127.0.0.1"
port = 7777
"#;

    let err = toml::from_str::<DaemonConfig>(toml_text).expect_err("missing relay should fail");

    assert!(err.to_string().contains("relay"));
}

#[test]
fn save_to_plain_filename_in_current_dir() {
    let _lock = CWD_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let original_cwd = std::env::current_dir().unwrap();
    let temp_dir = create_temp_dir();
    let _guard = CurrentDirGuard {
        original_cwd,
        temp_dir: temp_dir.clone(),
    };
    std::env::set_current_dir(&temp_dir).unwrap();

    let cfg = DaemonConfig::default();
    cfg.save_to(Path::new("daemon.toml")).unwrap();

    assert!(Path::new("daemon.toml").is_file());
    let parsed = DaemonConfig::load_from(Path::new("daemon.toml")).unwrap();
    assert_eq!(parsed.listen.port, cfg.listen.port);
}

struct CurrentDirGuard {
    original_cwd: PathBuf,
    temp_dir: PathBuf,
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

fn create_temp_dir() -> PathBuf {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "cli-pocket-config-roundtrip-{}-{}-{attempt}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if std::fs::create_dir(&path).is_ok() {
            return path;
        }
    }

    panic!("failed to create temp directory");
}
