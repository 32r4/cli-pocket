//! wasm-bindgen surface for cli-pocket client.
//!
//! Build: `wasm-pack build crates/client/client-core-wasm --target web`
//!
//! This module exposes [`CliPocketClient`] — a JS class that wraps
//! [`ClientSession`] plus the four platform adapters
//! ([`WsTransport`], [`IdbStore`], [`PerfClock`], [`CryptoRng`]).
//!
mod clock_perf;
mod kv_idb;
mod rng_crypto;
mod ws_transport;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use bytes::Bytes;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{
    ClientEvent, ClientIdentity, ClientResult, ClientSession, KeyValueStore, SessionBuilder,
    SessionConfig, SessionEndpoint,
};
use cli_pocket_proto::{Capabilities, ResumeToken, TerminalCreateParams};
use futures_channel::mpsc;
use futures_util::{future::LocalBoxFuture, StreamExt};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::clock_perf::PerfClock;
use crate::kv_idb::IdbStore;
use crate::rng_crypto::CryptoRng;
use crate::ws_transport::WsTransport;

#[cfg(test)]
mod tests {
    use super::*;
    use cli_pocket_client_core::session::SessionEndpoint;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn connect_surfaces_transport_errors_as_event() {
        let client = CliPocketClient::new().expect("construct client");

        client
            .connect(
                serde_json::json!({
                    "endpoint_url": "ws://127.0.0.1:9/ws/client",
                    "server_public_hex": "00".repeat(32),
                })
                .to_string()
                .into(),
            )
            .await
            .expect("connect should start session");

        let _connecting = client.next_event().await.expect("connecting event");
        let event = client.next_event().await.expect("disconnect event");
        let kind = js_sys::Reflect::get(&event, &JsValue::from_str("kind"))
            .expect("kind property")
            .as_string()
            .expect("kind string");

        assert_eq!(kind, "Disconnected");
    }

    #[test]
    fn terminal_created_event_serializes_for_web_bridge() {
        let value = event_to_json_value(&ClientEvent::TerminalCreated(
            cli_pocket_proto::TerminalInfo {
                terminal: cli_pocket_proto::TerminalId(uuid::Uuid::nil()),
                cols: 80,
                rows: 24,
                created_at_unix_ms: 1,
                label: None,
                attached_clients: 1,
            },
        ));

        assert_eq!(
            value,
            serde_json::json!({
                "kind": "TerminalCreated",
                "info": {
                    "terminal": uuid::Uuid::nil().to_string(),
                    "cols": 80,
                    "rows": 24,
                    "created_at_unix_ms": 1,
                    "label": null,
                    "attached_clients": 1,
                },
            })
        );
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn event_to_js_serializes_plain_js_objects() {
        let value = event_to_js(&ClientEvent::Connecting).expect("serialize event");

        assert!(js_sys::Reflect::has(&value, &JsValue::from_str("kind")).unwrap_or(false));
    }

    #[test]
    fn terminal_output_event_serializes_for_web_bridge() {
        let value = event_to_json_value(&ClientEvent::TerminalOutput {
            terminal_id: cli_pocket_proto::TerminalId(uuid::Uuid::nil()),
            stream_seq: cli_pocket_proto::StreamSeq(7),
            bytes: Bytes::from_static(b"hi"),
        });

        assert_eq!(
            value,
            serde_json::json!({
                "kind": "TerminalOutput",
                "terminal_id": uuid::Uuid::nil().to_string(),
                "stream_seq": 7,
                "bytes_b64": "aGk=",
            })
        );
    }

    #[test]
    fn terminal_exited_event_serializes_for_web_bridge() {
        let value = event_to_json_value(&ClientEvent::TerminalExited {
            terminal_id: cli_pocket_proto::TerminalId(uuid::Uuid::nil()),
            info: cli_pocket_proto::ExitInfo {
                code: Some(0),
                signal: None,
                at_unix_ms: 2,
            },
        });

        assert_eq!(
            value,
            serde_json::json!({
                "kind": "TerminalExited",
                "terminal_id": uuid::Uuid::nil().to_string(),
                "info": {
                    "code": 0,
                    "signal": null,
                    "at_unix_ms": 2,
                },
            })
        );
    }

    #[test]
    fn relay_config_parses_into_relay_endpoint() {
        let expected_host_id = uuid::Uuid::now_v7();
        let config = parse_connect_config_json(&serde_json::json!({
            "kind": "relay",
            "relayUrl": "wss://relay.example/ws/client",
            "hostId": expected_host_id.to_string(),
            "pskHex": "aa".repeat(32),
            "serverPublicHex": "bb".repeat(32),
            "resumeTokenHex": null,
        }))
        .expect("parse relay config");

        match config.endpoint {
            SessionEndpoint::Relay {
                url,
                host_id,
                psk_hex,
            } => {
                assert_eq!(url, "wss://relay.example/ws/client");
                assert_eq!(host_id.0, expected_host_id);
                assert_eq!(psk_hex, "aa".repeat(32));
            }
            other @ SessionEndpoint::Direct(_) => {
                panic!("expected relay endpoint, got {other:?}")
            }
        }
        assert_eq!(config.server_public_hex, "bb".repeat(32));
        assert!(config.resume_token_hex.is_none());
    }

    #[test]
    fn relay_config_rejects_missing_required_fields() {
        for value in [
            serde_json::json!({
                "kind": "relay",
                "hostId": uuid::Uuid::now_v7().to_string(),
                "pskHex": "aa".repeat(32),
                "serverPublicHex": "bb".repeat(32),
            }),
            serde_json::json!({
                "kind": "relay",
                "relayUrl": "wss://relay.example/ws/client",
                "pskHex": "aa".repeat(32),
                "serverPublicHex": "bb".repeat(32),
            }),
            serde_json::json!({
                "kind": "relay",
                "relayUrl": "wss://relay.example/ws/client",
                "hostId": uuid::Uuid::now_v7().to_string(),
                "serverPublicHex": "bb".repeat(32),
            }),
        ] {
            assert!(parse_connect_config_json(&value).is_err());
        }
    }
}

#[wasm_bindgen(start)]
pub fn _start() {
    console_error_panic_hook::set_once();
    let _ = tracing_wasm::try_set_as_global_default();
}

/// JS-facing client.
///
/// Owns the optional [`ClientSession`] and its event receiver behind
/// `Rc<RefCell<_>>` so wasm-bindgen `async fn(&self, ...)` methods can
/// borrow without lifetime grief.
#[wasm_bindgen]
pub struct CliPocketClient {
    inner: Rc<RefCell<Option<Rc<ClientSession>>>>,
    events: Rc<RefCell<Option<mpsc::Receiver<ClientEvent>>>>,
    kv: Rc<RefCell<Option<Rc<IdbStore>>>>,
    identity: Rc<RefCell<Option<ClientIdentity>>>,
}

/// JSON params consumed by [`CliPocketClient::create_terminal`].
///
/// Maps 1:1 onto [`TerminalCreateParams`]; all optional fields default to
/// sensible empty values so the JS caller only needs to supply `cols`/`rows`.
#[derive(Deserialize)]
struct JsCreateTerminalParams {
    cols: u16,
    rows: u16,
    #[serde(default)]
    cwd: Option<String>,
    /// Shell / command argv. Defaults to empty (server picks the default shell).
    #[serde(default)]
    cmd: Vec<String>,
    /// Environment overrides as `[[key, value], ...]`.
    #[serde(default)]
    env: Vec<(String, String)>,
    #[serde(default)]
    scrollback_bytes: Option<u32>,
}

/// JSON config consumed by [`CliPocketClient::connect`].
///
/// All fields originate in the web UI. Bytes are hex-encoded because
/// `serde-wasm-bindgen` does not round-trip raw `Uint8Array` cleanly
/// through `serde_json::from_str`.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JsConfig {
    Direct {
        #[serde(alias = "endpointUrl")]
        endpoint_url: String,
        #[serde(alias = "serverPublicHex")]
        server_public_hex: String,
        #[serde(default, alias = "resumeTokenHex")]
        resume_token_hex: Option<String>,
    },
    Relay {
        #[serde(alias = "relayUrl")]
        relay_url: String,
        #[serde(alias = "hostId")]
        host_id: String,
        #[serde(alias = "pskHex")]
        psk_hex: String,
        #[serde(alias = "serverPublicHex")]
        server_public_hex: String,
        #[serde(default, alias = "resumeTokenHex")]
        resume_token_hex: Option<String>,
    },
}

struct ParsedConnectConfig {
    endpoint: SessionEndpoint,
    server_public_hex: String,
    resume_token_hex: Option<String>,
}

#[wasm_bindgen]
impl CliPocketClient {
    /// Construct an idle client.
    ///
    /// No I/O happens until [`connect`](Self::connect) is called.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CliPocketClient, JsValue> {
        Ok(Self {
            inner: Rc::new(RefCell::new(None)),
            events: Rc::new(RefCell::new(None)),
            kv: Rc::new(RefCell::new(None)),
            identity: Rc::new(RefCell::new(None)),
        })
    }

    /// Open a session against `endpoint_url` using `server_public_hex`
    /// as the Noise-XK responder static key.
    ///
    #[wasm_bindgen]
    pub async fn connect(&self, config: JsValue) -> Result<(), JsValue> {
        let cfg = parse_connect_config(config)?;
        let server_public: [u8; 32] = hex::decode(&cfg.server_public_hex)
            .map_err(|e| JsValue::from_str(&format!("server_public_hex: {e}")))?
            .try_into()
            .map_err(|_| JsValue::from_str("server_public_hex must be 32 bytes"))?;
        let resume_token = parse_resume_token(cfg.resume_token_hex.as_deref())?;
        let kv = self.kv().await?;
        let identity = self.identity(&kv).await?;
        let endpoint = cfg.endpoint;
        let transport_url = match &endpoint {
            SessionEndpoint::Direct(url) | SessionEndpoint::Relay { url, .. } => url.clone(),
        };

        let builder = SessionBuilder::new(
            identity,
            SessionConfig {
                endpoint,
                server_public,
                resume_token,
                capabilities: Capabilities::NONE,
                backoff: (50, 1_000, 20),
            },
            PerfClock,
            CryptoRng,
            SharedIdbStore(Rc::clone(&kv)),
            move || {
                let url = transport_url.clone();
                Box::pin(async move { WsTransport::connect(&url, None).await })
            },
            WasmSpawner,
        );
        let (session, events) = builder.start();

        *self.inner.borrow_mut() = Some(Rc::new(session));
        *self.events.borrow_mut() = Some(events);
        Ok(())
    }

    /// Spawn a new terminal in the current session.
    ///
    /// `params_json` is a JSON object with fields:
    ///   `cols`, `rows`, `cwd?`, `cmd?` (string[]), `env?` ([[k,v],...]),
    ///   `scrollback_bytes?`.
    #[wasm_bindgen]
    pub async fn create_terminal(&self, params_json: String) -> Result<(), JsValue> {
        let session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?
            .clone();

        let js_params: JsCreateTerminalParams = serde_json::from_str(&params_json)
            .map_err(|e| JsValue::from_str(&format!("params_json: {e}")))?;

        let params = TerminalCreateParams {
            cols: js_params.cols,
            rows: js_params.rows,
            cwd: js_params.cwd,
            cmd: js_params.cmd,
            env: js_params.env,
            scrollback_bytes: js_params.scrollback_bytes,
        };

        session.create_terminal(params).await.map_err(js_error)
    }

    /// Send raw keystroke bytes to the active terminal.
    ///
    /// `data` is a `Uint8Array` on the JS side; wasm-bindgen copies it into a
    /// `Vec<u8>` before the await point so the caller does not need to keep
    /// the original buffer alive.
    ///
    /// Note: the `terminal_id` parameter is accepted for future multi-terminal
    /// support but currently ignored — only the single active terminal is
    /// addressed (the protocol only supports one at a time).
    #[wasm_bindgen]
    pub async fn send_input(&self, data: Vec<u8>) -> Result<(), JsValue> {
        let session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?
            .clone();

        let handle = session
            .terminal()
            .await
            .ok_or_else(|| JsValue::from_str("no active terminal"))?;

        handle
            .write_input(Bytes::from(data))
            .await
            .map_err(js_error)
    }

    /// Resize the active terminal.
    ///
    /// Note: the active terminal is always targeted (single-terminal protocol
    /// v1); a `terminal_id` parameter will be added when multi-terminal is
    /// supported.
    #[wasm_bindgen]
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), JsValue> {
        let session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?
            .clone();

        let handle = session
            .terminal()
            .await
            .ok_or_else(|| JsValue::from_str("no active terminal"))?;

        handle.resize(cols, rows).await.map_err(js_error)
    }

    /// Kill the active terminal.
    ///
    /// Note: the active terminal is always targeted (single-terminal protocol
    /// v1); a `terminal_id` parameter will be added when multi-terminal is
    /// supported.
    #[wasm_bindgen]
    pub async fn kill(&self) -> Result<(), JsValue> {
        let session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?
            .clone();

        let handle = session
            .terminal()
            .await
            .ok_or_else(|| JsValue::from_str("no active terminal"))?;

        handle.kill().await.map_err(js_error)
    }

    /// Await the next [`ClientEvent`].
    ///
    /// JS callers `await client.next_event()` in a loop; resolving with
    /// `null` signals the event stream is closed (the session ended).
    /// Plan I will swap this for a `ReadableStream`-style iterator once
    /// it has the consumer code to validate the contract.
    #[wasm_bindgen]
    pub async fn next_event(&self) -> Result<JsValue, JsValue> {
        let mut events = self
            .events
            .borrow_mut()
            .take()
            .ok_or_else(|| JsValue::from_str("not connected"))?;
        let event = events.next().await;
        *self.events.borrow_mut() = Some(events);

        event.map_or(Ok(JsValue::NULL), |event| event_to_js(&event))
    }

    /// Export the loaded identity as a base64-encoded string.
    ///
    /// The returned value can be stored externally and re-imported with
    /// [`import_identity`](Self::import_identity).  Requires that an identity
    /// has already been loaded (e.g. via [`connect`](Self::connect)).
    #[wasm_bindgen]
    pub fn export_identity(&self) -> Result<String, JsValue> {
        let identity_ref = self.identity.borrow();
        let id = identity_ref
            .as_ref()
            .ok_or_else(|| JsValue::from_str("no identity loaded"))?;
        let bytes = id.export_serialized().map_err(js_error)?;
        Ok(BASE64.encode(&bytes))
    }

    /// Import a base64-encoded identity blob and persist it to the KV store.
    ///
    /// After a successful import the in-memory identity cache is updated so
    /// subsequent calls to [`connect`](Self::connect) use the imported
    /// identity without reloading from storage.
    #[wasm_bindgen]
    pub async fn import_identity(&self, blob: String) -> Result<(), JsValue> {
        let raw = BASE64
            .decode(blob.as_bytes())
            .map_err(|e| JsValue::from_str(&format!("base64 decode: {e}")))?;
        let kv = self.kv().await?;
        ClientIdentity::import_serialized(kv.as_ref(), &raw)
            .await
            .map_err(js_error)?;
        // Reload from KV so the cached identity matches what was just persisted.
        let identity = ClientIdentity::load_or_create(kv.as_ref(), &CryptoRng)
            .await
            .map_err(js_error)?;
        *self.identity.borrow_mut() = Some(identity);
        Ok(())
    }

    /// Close the active session and drop all session state.
    ///
    /// Safe to call when not connected; subsequent [`connect`](Self::connect)
    /// calls will start a fresh session.
    #[wasm_bindgen]
    pub fn close(&self) -> Result<(), JsValue> {
        self.inner.borrow_mut().take();
        self.events.borrow_mut().take();
        Ok(())
    }
}

impl CliPocketClient {
    async fn kv(&self) -> Result<Rc<IdbStore>, JsValue> {
        if let Some(kv) = self.kv.borrow().as_ref() {
            return Ok(Rc::clone(kv));
        }

        let kv = Rc::new(IdbStore::open().await.map_err(js_error)?);
        *self.kv.borrow_mut() = Some(Rc::clone(&kv));
        Ok(kv)
    }

    async fn identity(&self, kv: &IdbStore) -> Result<ClientIdentity, JsValue> {
        if let Some(identity) = self.identity.borrow().as_ref() {
            return Ok(identity.clone());
        }

        let identity = ClientIdentity::load_or_create(kv, &CryptoRng)
            .await
            .map_err(js_error)?;
        *self.identity.borrow_mut() = Some(identity.clone());
        Ok(identity)
    }
}

#[derive(Clone, Copy)]
struct WasmSpawner;

impl SessionSpawner for WasmSpawner {
    fn spawn(&self, fut: LocalBoxFuture<'static, ()>) {
        spawn_local(fut);
    }
}

#[derive(Clone)]
struct SharedIdbStore(Rc<IdbStore>);

#[async_trait(?Send)]
impl KeyValueStore for SharedIdbStore {
    async fn get(&self, key: &str) -> ClientResult<Option<Vec<u8>>> {
        self.0.get(key).await
    }

    async fn put(&self, key: &str, value: &[u8]) -> ClientResult<()> {
        self.0.put(key, value).await
    }

    async fn delete(&self, key: &str) -> ClientResult<()> {
        self.0.delete(key).await
    }
}

fn parse_config(config: JsValue) -> Result<JsConfig, String> {
    if let Some(config_json) = config.as_string() {
        parse_config_json_str(&config_json)
    } else {
        serde_wasm_bindgen::from_value(config).map_err(|e| format!("config object: {e}"))
    }
}

fn parse_connect_config(config: JsValue) -> Result<ParsedConnectConfig, JsValue> {
    parse_connect_config_inner(parse_config(config)?).map_err(|e| JsValue::from_str(&e))
}

fn parse_config_json_str(config_json: &str) -> Result<JsConfig, String> {
    serde_json::from_str(config_json).map_err(|e| format!("config json: {e}"))
}

#[cfg(test)]
fn parse_connect_config_json(value: &serde_json::Value) -> Result<ParsedConnectConfig, String> {
    let cfg: JsConfig =
        serde_json::from_value(value.clone()).map_err(|e| format!("config json: {e}"))?;
    parse_connect_config_inner(cfg)
}

fn parse_connect_config_inner(cfg: JsConfig) -> Result<ParsedConnectConfig, String> {
    match cfg {
        JsConfig::Direct {
            endpoint_url,
            server_public_hex,
            resume_token_hex,
        } => Ok(ParsedConnectConfig {
            endpoint: SessionEndpoint::Direct(endpoint_url),
            server_public_hex,
            resume_token_hex,
        }),
        JsConfig::Relay {
            relay_url,
            host_id,
            psk_hex,
            server_public_hex,
            resume_token_hex,
        } => Ok(ParsedConnectConfig {
            endpoint: SessionEndpoint::Relay {
                url: relay_url,
                host_id: cli_pocket_proto::HostId(
                    uuid::Uuid::parse_str(&host_id).map_err(|e| format!("host_id: {e}"))?,
                ),
                psk_hex,
            },
            server_public_hex,
            resume_token_hex,
        }),
    }
}

fn parse_resume_token(value: Option<&str>) -> Result<Option<ResumeToken>, JsValue> {
    let Some(value) = value.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let bytes =
        hex::decode(value).map_err(|e| JsValue::from_str(&format!("resume_token_hex: {e}")))?;
    postcard::from_bytes(&bytes)
        .map(Some)
        .map_err(|e| JsValue::from_str(&format!("resume_token_hex: {e}")))
}

fn event_to_js(event: &ClientEvent) -> Result<JsValue, JsValue> {
    let value = event_to_json_value(event);

    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsValue::from_str(&format!("serialize event: {e}")))
}

fn event_to_json_value(event: &ClientEvent) -> serde_json::Value {
    match event {
        ClientEvent::Connecting => serde_json::json!({ "kind": "Connecting" }),
        ClientEvent::Connected { session_id } => {
            serde_json::json!({ "kind": "Connected", "session_id": session_id.0.to_string() })
        }
        ClientEvent::Disconnected { will_retry, reason } => {
            serde_json::json!({
                "kind": "Disconnected",
                "will_retry": will_retry,
                "reason": reason,
            })
        }
        ClientEvent::TerminalCreated(info) => serde_json::json!({
            "kind": "TerminalCreated",
            "info": {
                "terminal": info.terminal.0.to_string(),
                "cols": info.cols,
                "rows": info.rows,
                "created_at_unix_ms": info.created_at_unix_ms,
                "label": info.label,
                "attached_clients": info.attached_clients,
            },
        }),
        ClientEvent::TerminalOutput {
            terminal_id,
            stream_seq,
            bytes,
        } => serde_json::json!({
            "kind": "TerminalOutput",
            "terminal_id": terminal_id.0.to_string(),
            "stream_seq": stream_seq.0,
            "bytes_b64": BASE64.encode(bytes),
        }),
        ClientEvent::TerminalExited { terminal_id, info } => serde_json::json!({
            "kind": "TerminalExited",
            "terminal_id": terminal_id.0.to_string(),
            "info": {
                "code": info.code,
                "signal": info.signal,
                "at_unix_ms": info.at_unix_ms,
            },
        }),
        ClientEvent::Error(message) => {
            serde_json::json!({ "kind": "Error", "message": message })
        }
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
