use cli_pocket_daemon_core::session::SessionManager;
use cli_pocket_proto::{KillSignal, TerminalCreateParams};
use tokio::time::{sleep, timeout, Duration};

fn quick_exit_params() -> TerminalCreateParams {
    TerminalCreateParams {
        cols: 80,
        rows: 24,
        cwd: None,
        cmd: cmd_command(),
        env: Vec::new(),
    }
}

fn title_params(title: &str) -> TerminalCreateParams {
    TerminalCreateParams {
        cols: 80,
        rows: 24,
        cwd: None,
        cmd: title_command(title),
        env: Vec::new(),
    }
}

#[cfg(windows)]
fn cmd_command() -> Vec<String> {
    vec![
        "C:\\Windows\\System32\\cmd.exe".to_string(),
        "/C".to_string(),
        "echo done".to_string(),
    ]
}

#[cfg(windows)]
fn title_command(title: &str) -> Vec<String> {
    vec![
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        format!("[Console]::Out.Write([char]27 + ']0;{title}' + [char]7); Start-Sleep -Milliseconds 500"),
    ]
}

#[cfg(unix)]
fn cmd_command() -> Vec<String> {
    vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "echo done".to_string(),
    ]
}

#[cfg(unix)]
fn title_command(title: &str) -> Vec<String> {
    vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!("printf '\\033]0;{}\\007'; sleep 0.5", title),
    ]
}

#[tokio::test(flavor = "current_thread")]
async fn create_increases_count() {
    let mgr = SessionManager::new(8);
    assert_eq!(mgr.count(), 0);

    let info = mgr
        .create(quick_exit_params(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(mgr.count(), 1);
    assert_eq!(info.cols, 80);
    assert_eq!(info.rows, 24);

    // Wait for quick-exit command to finish and reaper to sweep.
    wait_for_reaper(&mgr, Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn attach_returns_none_for_unknown_id() {
    let mgr = SessionManager::new(8);
    let unknown = cli_pocket_proto::TerminalId::new();
    assert!(mgr.attach(&unknown).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn kill_unknown_terminal_returns_error() {
    let mgr = SessionManager::new(8);
    let unknown = cli_pocket_proto::TerminalId::new();
    let err = mgr.kill(&unknown, KillSignal::Term).await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("unknown") || msg.contains("not found"));
}

#[tokio::test(flavor = "current_thread")]
async fn reaper_removes_exited_terminal() {
    let mgr = SessionManager::new(8);
    mgr.create(quick_exit_params(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(mgr.count(), 1);

    // The echo command exits almost immediately; reaper polls at 100ms.
    wait_for_reaper(&mgr, Duration::from_secs(5)).await;
    assert_eq!(mgr.count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn enforces_max_terminals() {
    let mgr = SessionManager::new(1);

    // Create one terminal with quick-exit command.
    mgr.create(quick_exit_params(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(mgr.count(), 1);

    // While the terminal is still alive, creating a second should fail.
    let result = mgr.create(quick_exit_params(), 4 * 1024 * 1024).await;
    assert!(
        result.is_err(),
        "should not be able to exceed max_terminals"
    );
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("limit"), "expected limit error, got: {msg}");

    // Clean up — wait for reaper.
    wait_for_reaper(&mgr, Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_terminal_info() {
    let mgr = SessionManager::new(8);

    mgr.create(quick_exit_params(), 4 * 1024 * 1024)
        .await
        .unwrap();

    let list = mgr.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].cols, 80);
    assert_eq!(list[0].rows, 24);

    wait_for_reaper(&mgr, Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_terminal_title_as_label() {
    let mgr = SessionManager::new(8);

    mgr.create(title_params("project shell"), 4 * 1024 * 1024)
        .await
        .unwrap();

    let title_seen = timeout(Duration::from_secs(5), async {
        loop {
            let title = mgr
                .list()
                .first()
                .and_then(|terminal| terminal.label.clone());
            if title.as_deref() == Some("project shell") {
                break true;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or(false);

    assert!(title_seen, "terminal title should be published as label");
    wait_for_reaper(&mgr, Duration::from_secs(5)).await;
}

/// Poll `mgr.count()` until it reaches 0, with a timeout.
async fn wait_for_reaper(mgr: &SessionManager, timeout_duration: Duration) {
    let _ = timeout(timeout_duration, async {
        loop {
            if mgr.count() == 0 {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
}
