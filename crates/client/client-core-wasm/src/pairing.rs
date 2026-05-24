//! SPAKE2 client-side pairing flow exposed to JavaScript.
//!
//! The daemon (server) exposes a `/pair` WebSocket path which:
//!   1. Sends its SPAKE2 host outbound bytes (raw binary WS frame).
//!   2. Receives the client's SPAKE2 outbound bytes.
//!   3. Receives the client's static public key encrypted with the SPAKE2 PSK.
//!   4. Sends the daemon's own static public key encrypted with the PSK.
//!
//! This module mirrors that from the client side.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use cli_pocket_client_core::{ClientIdentity, Transport};
use cli_pocket_crypto::Spake2Side;
use js_sys::Object;
use wasm_bindgen::prelude::*;

use crate::kv_idb::IdbStore;
use crate::rng_crypto::CryptoRng;
use crate::ws_transport::WsTransport;

/// Identity strings must match the daemon exactly.
const PAIRING_HOST_ID: &[u8] = b"cli-pocket pairing host v1";
const PAIRING_CLIENT_ID: &[u8] = b"cli-pocket pairing client v1";
/// Zero nonce — safe because the PSK is single-use.
const PAIRING_AEAD_NONCE: [u8; 12] = [0_u8; 12];

/// Drive the SPAKE2 client side against a daemon's pairing endpoint.
///
/// `pairing_url` is the daemon's `/pair` WebSocket URL.
///
/// On success returns a JS object: `{ server_public_hex, client_public_hex }`.
/// The browser caller persists the result and feeds `server_public_hex`
/// into `CliPocketClient.connect()` later.
#[wasm_bindgen]
pub async fn client_pair_with_code(pairing_url: String, code: String) -> Result<JsValue, JsValue> {
    // 1. Open a WebSocket to the daemon's pairing endpoint.
    let mut transport = WsTransport::connect(&pairing_url, None)
        .await
        .map_err(|e| JsValue::from_str(&format!("connect: {e}")))?;

    // 2. Receive the daemon's SPAKE2 host outbound bytes.
    let daemon_sp_bytes = transport
        .recv()
        .await
        .map_err(|e| JsValue::from_str(&format!("recv spake2 host msg: {e}")))?
        .ok_or_else(|| JsValue::from_str("daemon closed before sending SPAKE2 message"))?;

    // 3. Start the SPAKE2 client side using the shared code.
    let sp = Spake2Side::start_client(code.as_bytes(), PAIRING_HOST_ID, PAIRING_CLIENT_ID);

    // 4. Send our SPAKE2 outbound bytes to the daemon.
    transport
        .send(sp.outbound().to_vec())
        .await
        .map_err(|e| JsValue::from_str(&format!("send spake2 client msg: {e}")))?;

    // 5. Finish SPAKE2 → shared PSK.
    let outcome = sp
        .finish(&daemon_sp_bytes)
        .map_err(|e| JsValue::from_str(&format!("spake2 finish: {e}")))?;

    // 6. Load or create the client's persistent identity from IndexedDB.
    let kv = IdbStore::open()
        .await
        .map_err(|e| JsValue::from_str(&format!("idb open: {e}")))?;
    let identity = ClientIdentity::load_or_create(&kv, &CryptoRng)
        .await
        .map_err(|e| JsValue::from_str(&format!("identity: {e}")))?;
    let client_pk_bytes: [u8; 32] = identity.keypair.public;

    // 7. Encrypt our static public key with the SPAKE2 PSK and send it.
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&outcome.psk));
    let nonce = Nonce::from_slice(&PAIRING_AEAD_NONCE);
    let client_pk_ct = cipher
        .encrypt(nonce, client_pk_bytes.as_ref())
        .map_err(|e| JsValue::from_str(&format!("encrypt client pk: {e}")))?;
    transport
        .send(client_pk_ct)
        .await
        .map_err(|e| JsValue::from_str(&format!("send client pk: {e}")))?;

    // 8. Receive the daemon's encrypted static public key ack.
    let server_pk_ct = transport
        .recv()
        .await
        .map_err(|e| JsValue::from_str(&format!("recv server pk ct: {e}")))?
        .ok_or_else(|| JsValue::from_str("daemon closed before sending server pk ack"))?;

    let server_pk_vec = cipher
        .decrypt(nonce, server_pk_ct.as_ref())
        .map_err(|e| JsValue::from_str(&format!("decrypt server pk: {e}")))?;

    if server_pk_vec.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "expected 32-byte server pk, got {}",
            server_pk_vec.len()
        )));
    }

    // 9. Close the transport.
    transport
        .close()
        .await
        .map_err(|e| JsValue::from_str(&format!("close: {e}")))?;

    // 10. Return JS object { server_public_hex, client_public_hex }.
    let server_public_hex = hex::encode(&server_pk_vec);
    let client_public_hex = hex::encode(client_pk_bytes);

    let obj = Object::new();
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("server_public_hex"),
        &JsValue::from_str(&server_public_hex),
    )
    .map_err(|e| JsValue::from_str(&format!("set server_public_hex: {e:?}")))?;
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("client_public_hex"),
        &JsValue::from_str(&client_public_hex),
    )
    .map_err(|e| JsValue::from_str(&format!("set client_public_hex: {e:?}")))?;

    Ok(obj.into())
}
