use base64::Engine as _;
use bytes::Bytes;
use cli_pocket_client_core::{
    ClientIdentity, KeyValueStore, SessionBuilder, SessionConfig, SessionEndpoint, TerminalSnapshot,
};
use cli_pocket_proto::{
    ResumeToken, ServerConfig, StreamSeq, TerminalCreateParams, TerminalId, TerminalInfo,
};
use cli_pocket_tauri_bindings::{
    FileKvStore, OsRandom, SessionHandle, TokioClock, TokioWsTransport,
};
use serde::Deserialize;
use tauri::async_runtime;

const DAEMON_REGISTRY_KEY: &str = "daemon_registry_v1";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ConnectArgs {
    Direct {
        #[serde(alias = "endpointUrl")]
        endpoint_url: String,
        #[serde(default, alias = "resumeTokenHex")]
        resume_token_hex: Option<String>,
    },
    Relay {
        #[serde(alias = "relayUrl")]
        relay_url: String,
        #[serde(alias = "serverId")]
        server_id: String,
        #[serde(alias = "pskHex")]
        psk_hex: String,
        #[serde(alias = "serverPublicHex")]
        server_public_hex: String,
        #[serde(default, alias = "resumeTokenHex")]
        resume_token_hex: Option<String>,
    },
}

struct ParsedConnectArgs {
    endpoint: SessionEndpoint,
    transport_url: String,
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
}

pub async fn connect(
    session: SessionHandle,
    kv: FileKvStore,
    config: serde_json::Value,
    event_channel: String,
    direct_ws_subprotocol: Option<&'static str>,
) -> Result<(), String> {
    let config: ConnectArgs = serde_json::from_value(config).map_err(|error| error.to_string())?;
    let config = parse_connect_args(config)?;
    let resume_token = parse_resume_token(config.resume_token_hex.as_deref())?;
    let identity = load_identity(kv.clone()).await?;
    let endpoint = config.endpoint;
    let transport_url = config.transport_url;
    let ws_subprotocol = effective_ws_subprotocol(&endpoint, direct_ws_subprotocol);

    session
        .connect(event_channel, move |spawner| {
            SessionBuilder::new(
                identity,
                SessionConfig {
                    endpoint,
                    resume_token,
                    backoff: (50, 1_000, 20),
                },
                TokioClock,
                OsRandom,
                kv.clone(),
                move || {
                    let url = transport_url.clone();
                    Box::pin(async move { TokioWsTransport::connect(&url, ws_subprotocol).await })
                },
                spawner.clone(),
            )
        })
        .await
}

pub async fn create_terminal(
    session: SessionHandle,
    params: serde_json::Value,
) -> Result<(), String> {
    let params: CreateTerminalArgs =
        serde_json::from_value(params).map_err(|error| error.to_string())?;

    session
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
        })
        .await
}

pub async fn open_terminal(
    session: SessionHandle,
    terminal_id: String,
) -> Result<serde_json::Value, String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    session
        .open_terminal(terminal_id)
        .await
        .map(|snapshot| serialize_terminal_snapshot(&snapshot))
}

pub async fn list_terminals(session: SessionHandle) -> Result<Vec<serde_json::Value>, String> {
    session.list_terminals().await.map(|terminals| {
        terminals
            .iter()
            .map(serialize_terminal_info)
            .collect::<Vec<_>>()
    })
}

pub async fn read_history(
    session: SessionHandle,
    terminal_id: String,
    before: Option<u64>,
    max_bytes: u32,
) -> Result<serde_json::Value, String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    session
        .read_history(terminal_id, before.map(StreamSeq), max_bytes)
        .await
        .map(|page| {
            serde_json::json!({
                "terminal_id": page.terminal_id.0.to_string(),
                "start_seq": page.start_seq.0,
                "end_seq": page.end_seq.0,
                "bytes_b64": base64::engine::general_purpose::STANDARD.encode(&page.bytes),
            })
        })
}

pub async fn get_server_config(session: SessionHandle) -> Result<serde_json::Value, String> {
    session
        .get_server_config()
        .await
        .map(|config| serialize_server_config(&config))
}

pub async fn set_server_config(
    session: SessionHandle,
    config: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let config: ServerConfig = serde_json::from_value(config).map_err(|error| error.to_string())?;
    session
        .set_server_config(config)
        .await
        .map(|config| serialize_server_config(&config))
}

pub async fn send_input(
    session: SessionHandle,
    terminal_id: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    session.send_input(terminal_id, Bytes::from(bytes)).await
}

pub async fn resize(
    session: SessionHandle,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    session.resize(terminal_id, cols, rows).await
}

pub async fn kill(
    session: SessionHandle,
    terminal_id: String,
    signal: Option<String>,
) -> Result<(), String> {
    let terminal_id = parse_terminal_id(&terminal_id)?;
    validate_signal(signal.as_deref())?;
    session.kill(terminal_id).await
}

pub async fn export_identity(kv: FileKvStore) -> Result<Vec<u8>, String> {
    let identity = load_identity(kv).await?;
    identity
        .export_serialized()
        .map_err(|error| error.to_string())
}

pub async fn import_identity(kv: FileKvStore, blob: Vec<u8>) -> Result<(), String> {
    async_runtime::spawn_blocking(move || {
        async_runtime::block_on(ClientIdentity::import_serialized(&kv, &blob))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn close(session: SessionHandle) -> Result<(), String> {
    session.shutdown().await;
    Ok(())
}

pub async fn load_daemon_registry(kv: FileKvStore) -> Result<Option<serde_json::Value>, String> {
    let Some(bytes) = async_runtime::spawn_blocking(move || {
        async_runtime::block_on(kv.get(DAEMON_REGISTRY_KEY)).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??
    else {
        return Ok(None);
    };

    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub async fn save_daemon_registry(kv: FileKvStore, state: serde_json::Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(&state).map_err(|error| error.to_string())?;

    async_runtime::spawn_blocking(move || {
        async_runtime::block_on(kv.put(DAEMON_REGISTRY_KEY, &bytes))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn parse_resume_token(value: Option<&str>) -> Result<Option<ResumeToken>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = hex::decode(value).map_err(|error| format!("resume_token_hex: {error}"))?;
    postcard::from_bytes(&bytes).map_err(|error| format!("resume_token_hex: {error}"))
}

fn parse_connect_args(config: ConnectArgs) -> Result<ParsedConnectArgs, String> {
    match config {
        ConnectArgs::Direct {
            endpoint_url,
            resume_token_hex,
        } => Ok(ParsedConnectArgs {
            endpoint: SessionEndpoint::Direct(endpoint_url.clone()),
            transport_url: endpoint_url,
            resume_token_hex,
        }),
        ConnectArgs::Relay {
            relay_url,
            server_id,
            psk_hex,
            server_public_hex,
            resume_token_hex,
        } => Ok(ParsedConnectArgs {
            endpoint: SessionEndpoint::Relay {
                url: relay_url.clone(),
                server_id: cli_pocket_proto::ServerId(
                    uuid::Uuid::parse_str(&server_id)
                        .map_err(|error| format!("server_id: {error}"))?,
                ),
                psk_hex,
                server_public: hex::decode(&server_public_hex)
                    .map_err(|error| format!("server_public_hex: {error}"))?
                    .try_into()
                    .map_err(|_| "server_public_hex must be 32 bytes".to_owned())?,
            },
            transport_url: relay_url,
            resume_token_hex,
        }),
    }
}

fn effective_ws_subprotocol(
    endpoint: &SessionEndpoint,
    direct_ws_subprotocol: Option<&'static str>,
) -> Option<&'static str> {
    match endpoint {
        SessionEndpoint::Direct(_) => direct_ws_subprotocol,
        SessionEndpoint::Relay { .. } => None,
    }
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

fn serialize_terminal_info(info: &TerminalInfo) -> serde_json::Value {
    serde_json::json!({
        "terminal": info.terminal.0.to_string(),
        "cols": info.cols,
        "rows": info.rows,
        "created_at_unix_ms": info.created_at_unix_ms,
        "label": info.label,
        "attached_clients": info.attached_clients,
    })
}

fn serialize_terminal_snapshot(snapshot: &TerminalSnapshot) -> serde_json::Value {
    serde_json::json!({
        "info": serialize_terminal_info(&snapshot.info),
        "start_seq": snapshot.start_seq.0,
        "end_seq": snapshot.end_seq.0,
        "render_prefix_b64": base64::engine::general_purpose::STANDARD.encode(snapshot.render_prefix.as_bytes()),
        "snapshot_bytes_b64": base64::engine::general_purpose::STANDARD.encode(&snapshot.bytes),
    })
}

fn serialize_server_config(config: &ServerConfig) -> serde_json::Value {
    serde_json::json!({
        "scrollback_bytes": config.scrollback_bytes,
    })
}

async fn load_identity(kv: FileKvStore) -> Result<ClientIdentity, String> {
    async_runtime::spawn_blocking(move || {
        async_runtime::block_on(ClientIdentity::load_or_create(&kv, &OsRandom))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{
        effective_ws_subprotocol, parse_connect_args, parse_resume_token, parse_terminal_id,
        validate_signal, ConnectArgs,
    };
    use cli_pocket_client_core::SessionEndpoint;
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

    #[test]
    fn parse_connect_args_accepts_relay_union() {
        let server_id = uuid::Uuid::now_v7();

        let parsed = parse_connect_args(ConnectArgs::Relay {
            relay_url: "wss://relay.example/ws/client".to_owned(),
            server_id: server_id.to_string(),
            psk_hex: "aa".repeat(32),
            server_public_hex: "bb".repeat(32),
            resume_token_hex: None,
        })
        .expect("parse relay connect args");

        match parsed.endpoint {
            SessionEndpoint::Relay {
                url,
                server_id: parsed_server_id,
                psk_hex,
                server_public,
            } => {
                assert_eq!(url, "wss://relay.example/ws/client");
                assert_eq!(parsed_server_id.0, server_id);
                assert_eq!(psk_hex, "aa".repeat(32));
                assert_eq!(server_public, [0xbb; 32]);
            }
            other @ SessionEndpoint::Direct(_) => {
                panic!("expected relay endpoint, got {other:?}")
            }
        }
    }

    #[test]
    fn effective_ws_subprotocol_only_applies_to_direct_connections() {
        assert_eq!(
            effective_ws_subprotocol(
                &SessionEndpoint::Direct("ws://127.0.0.1:17842/session".to_owned()),
                Some("cli-pocket-server/v1"),
            ),
            Some("cli-pocket-server/v1")
        );

        assert_eq!(
            effective_ws_subprotocol(
                &SessionEndpoint::Relay {
                    url: "wss://relay.example/ws/client".to_owned(),
                    server_id: cli_pocket_proto::ServerId(uuid::Uuid::nil()),
                    psk_hex: "aa".repeat(32),
                    server_public: [0xbb; 32],
                },
                Some("cli-pocket-server/v1"),
            ),
            None
        );
    }
}
