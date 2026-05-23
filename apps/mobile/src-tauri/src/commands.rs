use crate::state::AppState;
use bytes::Bytes;
use cli_pocket_client_core::{ClientIdentity, SessionBuilder, SessionConfig, SessionEndpoint};
use cli_pocket_proto::Capabilities;
use cli_pocket_proto::{ResumeToken, TerminalCreateParams, TerminalId};
use cli_pocket_tauri_bindings::{OsRandom, TokioClock, TokioWsTransport};
use serde::Deserialize;
use tauri::async_runtime;
use tauri::State;

#[derive(Debug, Deserialize)]
struct ConnectArgs {
    #[serde(alias = "endpointUrl")]
    endpoint_url: String,
    #[serde(alias = "serverPublicHex")]
    server_public_hex: String,
    #[serde(default, alias = "resumeTokenHex")]
    resume_token_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTerminalArgs {
    cols: u16,
    rows: u16,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default, alias = "scrollbackBytes")]
    scrollback_bytes: Option<u32>,
}

#[tauri::command]
pub async fn cli_pocket_connect(
    state: State<'_, AppState>,
    config: serde_json::Value,
) -> Result<(), String> {
    let config: ConnectArgs = serde_json::from_value(config).map_err(|error| error.to_string())?;
    let server_public: [u8; 32] = hex::decode(&config.server_public_hex)
        .map_err(|error| format!("server_public_hex: {error}"))?
        .try_into()
        .map_err(|_| "server_public_hex must be 32 bytes".to_owned())?;
    let resume_token = parse_resume_token(config.resume_token_hex.as_deref())?;
    let session = state.session.clone();
    let kv = state.kv.clone();
    let identity = async_runtime::block_on(ClientIdentity::load_or_create(&kv, &OsRandom))
        .map_err(|error| error.to_string())?;
    let endpoint = config.endpoint_url.clone();
    let transport_url = config.endpoint_url;

    session
        .connect(move |spawner| {
            SessionBuilder::new(
                identity,
                SessionConfig {
                    endpoint: SessionEndpoint::Direct(endpoint),
                    server_public,
                    resume_token,
                    capabilities: Capabilities::NONE,
                    backoff: (50, 1_000, 20),
                },
                TokioClock,
                OsRandom,
                kv.clone(),
                move || {
                    let url = transport_url.clone();
                    Box::pin(async move {
                        TokioWsTransport::connect(&url, Some("cli-pocket-host/v1")).await
                    })
                },
                spawner.clone(),
            )
        })
        .await
}

#[tauri::command]
pub async fn cli_pocket_create_terminal(
    state: State<'_, AppState>,
    params: serde_json::Value,
) -> Result<(), String> {
    let params: CreateTerminalArgs =
        serde_json::from_value(params).map_err(|error| error.to_string())?;

    state
        .session
        .create_terminal(TerminalCreateParams {
            cols: params.cols,
            rows: params.rows,
            cwd: params.cwd,
            cmd: if params.cmd.is_empty() {
                params.shell.into_iter().collect()
            } else {
                params.cmd
            },
            env: params.env.into_iter().collect(),
            scrollback_bytes: params.scrollback_bytes,
        })
        .await
}

#[tauri::command]
pub async fn cli_pocket_send_input(
    state: State<'_, AppState>,
    terminal_id: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    state
        .session
        .send_input(terminal_id, Bytes::from(bytes))
        .await
}

#[tauri::command]
pub async fn cli_pocket_resize(
    state: State<'_, AppState>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    state.session.resize(terminal_id, cols, rows).await
}

#[tauri::command]
pub async fn cli_pocket_kill(
    state: State<'_, AppState>,
    terminal_id: String,
    signal: Option<String>,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    validate_signal(signal.as_deref())?;
    state.session.kill(terminal_id).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn cli_pocket_export_identity(state: State<'_, AppState>) -> Result<Vec<u8>, String> {
    let identity = async_runtime::block_on(ClientIdentity::load_or_create(&state.kv, &OsRandom))
        .map_err(|error| error.to_string())?;
    identity
        .export_serialized()
        .map_err(|error| error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn cli_pocket_import_identity(state: State<'_, AppState>, blob: Vec<u8>) -> Result<(), String> {
    async_runtime::block_on(ClientIdentity::import_serialized(&state.kv, &blob))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cli_pocket_close(state: State<'_, AppState>) -> Result<(), String> {
    state.session.shutdown().await;
    Ok(())
}

fn parse_resume_token(value: Option<&str>) -> Result<Option<ResumeToken>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = hex::decode(value).map_err(|error| format!("resume_token_hex: {error}"))?;
    postcard::from_bytes(&bytes).map_err(|error| format!("resume_token_hex: {error}"))
}

fn parse_terminal_id(value: &str) -> Result<TerminalId, String> {
    let uuid = uuid::Uuid::parse_str(value).map_err(|error| format!("terminal_id: {error}"))?;
    Ok(TerminalId(uuid))
}

fn validate_signal(signal: Option<&str>) -> Result<(), String> {
    let Some(signal) = signal else {
        return Ok(());
    };

    match signal {
        "TERM" | "HUP" | "KILL" => Ok(()),
        _ => Err(format!("unsupported signal: {signal}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_resume_token, parse_terminal_id, validate_signal};
    use cli_pocket_proto::{ResumeAttachment, ResumeToken, SessionId, StreamSeq, TerminalId};

    #[test]
    fn parse_resume_token_accepts_postcard_hex() {
        let token = ResumeToken {
            session_id: SessionId::new(),
            attachments: vec![ResumeAttachment {
                terminal: TerminalId::new(),
                last_seq: StreamSeq(7),
            }],
        };
        let encoded = hex::encode(postcard::to_allocvec(&token).expect("serialize resume token"));

        let parsed = parse_resume_token(Some(&encoded)).expect("parse resume token");

        assert_eq!(parsed, Some(token));
    }

    #[test]
    fn parse_resume_token_ignores_empty_string() {
        assert_eq!(parse_resume_token(Some("")).expect("empty token"), None);
    }

    #[test]
    fn parse_terminal_id_requires_uuid() {
        let terminal = TerminalId::new();
        let parsed =
            parse_terminal_id(&terminal.0.to_string()).expect("parse generated terminal id");

        assert_eq!(parsed, terminal);
        assert!(parse_terminal_id("not-a-uuid").is_err());
    }

    #[test]
    fn validate_signal_allows_known_values() {
        assert!(validate_signal(None).is_ok());
        assert!(validate_signal(Some("TERM")).is_ok());
        assert!(validate_signal(Some("HUP")).is_ok());
        assert!(validate_signal(Some("KILL")).is_ok());
        assert_eq!(
            validate_signal(Some("INT")).expect_err("reject unsupported signal"),
            "unsupported signal: INT"
        );
    }
}
