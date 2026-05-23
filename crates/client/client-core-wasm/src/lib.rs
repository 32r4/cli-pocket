//! wasm-bindgen surface for cli-pocket client.
//!
//! Build: `wasm-pack build crates/client/client-core-wasm --target web`
//!
//! This module exposes [`CliPocketClient`] — a JS class that wraps
//! [`ClientSession`] plus the four platform adapters
//! ([`WsTransport`], [`IdbStore`], [`PerfClock`], [`CryptoRng`]).
//!
//! The surface is intentionally a stub: per Plan F Task F13 the actual
//! session wiring (transport factory, identity persistence, event drain
//! into JS) lands in Plan I once the web UI is the consumer. Each method
//! either returns a typed "not yet implemented" error or performs the
//! tiny piece of work that the JS contract genuinely depends on (e.g.
//! parsing the config JSON in `connect`).

mod clock_perf;
mod kv_idb;
mod rng_crypto;
mod ws_transport;

use cli_pocket_client_core::{ClientEvent, ClientSession};
use futures_channel::mpsc;
use serde::Deserialize;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

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
    inner: Rc<RefCell<Option<ClientSession>>>,
    events: Rc<RefCell<Option<mpsc::Receiver<ClientEvent>>>>,
}

/// JSON config consumed by [`CliPocketClient::connect`].
///
/// All fields originate in the web UI. Bytes are hex-encoded because
/// `serde-wasm-bindgen` does not round-trip raw `Uint8Array` cleanly
/// through `serde_json::from_str`.
#[derive(Deserialize)]
struct JsConfig {
    /// `wss://…` (direct) or `wss://relay/...` (relay-mediated).
    endpoint_url: String,
    /// Hex-encoded 32-byte X25519 server static public key.
    server_public_hex: String,
    /// Optional hex-encoded resume token from a previous session.
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
        })
    }

    /// Open a session against `endpoint_url` using `server_public_hex`
    /// as the Noise-XK responder static key.
    ///
    /// Validates the config shape eagerly so JS-side mistakes surface at
    /// the call site. The actual session bring-up (building a
    /// [`SessionBuilder`] over [`WsTransport`] / [`IdbStore`] /
    /// [`PerfClock`] / [`CryptoRng`] and storing the resulting
    /// [`ClientSession`] in `self.inner`) is wired in Plan I.
    #[wasm_bindgen]
    pub async fn connect(&self, config_json: String) -> Result<(), JsValue> {
        let cfg: JsConfig = serde_json::from_str(&config_json)
            .map_err(|e| JsValue::from_str(&format!("config json: {e}")))?;
        let server_public: [u8; 32] = hex::decode(&cfg.server_public_hex)
            .map_err(|e| JsValue::from_str(&format!("hex: {e}")))?
            .try_into()
            .map_err(|_| JsValue::from_str("server_public_hex must be 32 bytes"))?;
        // The resume token (if any) is opaque proto bytes; the codec lives in
        // `cli-pocket-proto` and will be threaded through `SessionConfig` in
        // Plan I. We validate the hex shape here so the JS side fails fast.
        let resume_bytes = cfg
            .resume_token_hex
            .as_deref()
            .map(hex::decode)
            .transpose()
            .map_err(|e| JsValue::from_str(&format!("resume hex: {e}")))?;
        let _ = (cfg.endpoint_url, server_public, resume_bytes);
        Err(JsValue::from_str("Plan F13 wires this in Plan I"))
    }

    /// Spawn a new terminal in the current session.
    ///
    /// `params_json` is a JSON-serialized [`TerminalCreateParams`]. The
    /// JS side already speaks JSON; we keep the wire as JSON rather than
    /// `Uint8Array` so xterm.js can call this with a plain object.
    #[wasm_bindgen]
    pub async fn create_terminal(&self, _params_json: String) -> Result<(), JsValue> {
        let _session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?;
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Send raw keystroke bytes to the active terminal.
    ///
    /// Bytes are copied off the JS heap before the await point, so the
    /// caller does not need to keep the `Uint8Array` alive.
    #[wasm_bindgen]
    pub async fn send_input(&self, _data: Vec<u8>) -> Result<(), JsValue> {
        let _session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?;
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Resize the active terminal.
    #[wasm_bindgen]
    pub async fn resize(&self, _cols: u16, _rows: u16) -> Result<(), JsValue> {
        let _session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?;
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Kill the active terminal.
    #[wasm_bindgen]
    pub async fn kill(&self) -> Result<(), JsValue> {
        let _session = self
            .inner
            .borrow()
            .as_ref()
            .ok_or_else(|| JsValue::from_str("not connected"))?;
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Await the next [`ClientEvent`].
    ///
    /// JS callers `await client.next_event()` in a loop; resolving with
    /// `null` signals the event stream is closed (the session ended).
    /// Plan I will swap this for a `ReadableStream`-style iterator once
    /// it has the consumer code to validate the contract.
    #[wasm_bindgen]
    pub async fn next_event(&self) -> Result<JsValue, JsValue> {
        if self.events.borrow().is_none() {
            return Err(JsValue::from_str("not connected"));
        }
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Export the persisted identity as JSON bytes.
    ///
    /// Mirrors [`ClientIdentity::export_serialized`]; round-trips through
    /// [`import_identity`](Self::import_identity).
    #[wasm_bindgen]
    pub async fn export_identity(&self) -> Result<Vec<u8>, JsValue> {
        Err(JsValue::from_str("not yet implemented"))
    }

    /// Import a previously exported identity.
    #[wasm_bindgen]
    pub async fn import_identity(&self, _bytes: Vec<u8>) -> Result<(), JsValue> {
        Err(JsValue::from_str("not yet implemented"))
    }
}
