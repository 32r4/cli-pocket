//! `web-sys` WebSocket -> [`Transport`] adapter.
//!
//! The browser exposes WebSocket via event-driven callbacks. We bridge those
//! into an async [`Transport`] by:
//!   * registering `onopen` / `onmessage` / `onclose` / `onerror` callbacks,
//!   * forwarding inbound `ArrayBuffer` messages to an unbounded mpsc,
//!   * blocking [`WsTransport::connect`] on a oneshot until `onopen` fires,
//!   * encoding outbound bytes via `WebSocket::send_with_u8_array`.

use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, ClientResult, Transport};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use js_sys::ArrayBuffer;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// Item flowing from the WebSocket event-loop callbacks into `recv()`:
///   * `Ok(Some(bytes))` — a binary frame arrived,
///   * `Ok(None)` — the socket closed cleanly (terminates the stream),
///   * `Err(_)` — the socket reported an error.
type Incoming = ClientResult<Option<Vec<u8>>>;

/// A WebSocket adapter that implements [`Transport`].
///
/// Owns the underlying [`WebSocket`] plus the JS-side [`Closure`]s so they
/// outlive the socket. Dropping a [`Closure`] frees the JS reference, which
/// would turn the next callback into a use-after-free; storing them on the
/// struct keeps them alive until the transport itself is dropped.
#[allow(dead_code)] // Wired into the public JS API by Task F13.
pub struct WsTransport {
    ws: WebSocket,
    rx: mpsc::UnboundedReceiver<Incoming>,
    // Closures must outlive the WebSocket; keep them alive on the struct.
    _on_open: Closure<dyn FnMut()>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
}

impl WsTransport {
    /// Open a WebSocket to `url` and wait for the `onopen` event before
    /// returning. An optional subprotocol is offered during the handshake.
    #[allow(dead_code)] // Wired into the public JS API by Task F13.
    pub async fn connect(url: &str, subprotocol: Option<&str>) -> ClientResult<Self> {
        let ws = match subprotocol {
            Some(p) => WebSocket::new_with_str(url, p),
            None => WebSocket::new(url),
        }
        .map_err(|e| ClientError::Transport(format!("ws new: {e:?}")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        // Resolve once when `onopen` fires.
        let (open_tx, open_rx) = oneshot::channel::<ClientResult<()>>();
        let open_tx = Rc::new(RefCell::new(Some(open_tx)));
        let on_open = {
            let open_tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move || {
                if let Some(tx) = open_tx.borrow_mut().take() {
                    let _ = tx.send(Ok(()));
                }
            }) as Box<dyn FnMut()>)
        };
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        // Inbound channel: messages, close, and errors all funnel through it
        // so `recv()` observes them in order.
        let (tx, rx) = mpsc::unbounded::<Incoming>();

        let on_message = {
            let tx = tx.clone();
            Closure::wrap(Box::new(move |evt: MessageEvent| {
                if let Ok(buf) = evt.data().dyn_into::<ArrayBuffer>() {
                    let arr = js_sys::Uint8Array::new(&buf);
                    let mut out = vec![0u8; arr.length() as usize];
                    arr.copy_to(&mut out);
                    let _ = tx.unbounded_send(Ok(Some(out)));
                }
                // Non-ArrayBuffer payloads (e.g. text frames) are ignored:
                // the wire protocol is binary-only.
            }) as Box<dyn FnMut(MessageEvent)>)
        };
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let on_close = {
            let tx = tx.clone();
            let open_tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |_evt: CloseEvent| {
                if let Some(tx) = open_tx.borrow_mut().take() {
                    let _ = tx.send(Err(ClientError::Transport("ws closed before open".into())));
                }
                // `Ok(None)` matches the `Option<Vec<u8>>` end-of-stream
                // contract on `Transport::recv`.
                let _ = tx.unbounded_send(Ok(None));
            }) as Box<dyn FnMut(CloseEvent)>)
        };
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let on_error = {
            let tx = tx.clone();
            let open_tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |_evt: ErrorEvent| {
                if let Some(tx) = open_tx.borrow_mut().take() {
                    let _ = tx.send(Err(ClientError::Transport("ws error".into())));
                }
                let _ = tx.unbounded_send(Err(ClientError::Transport("ws error".into())));
            }) as Box<dyn FnMut(ErrorEvent)>)
        };
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        // Block until the handshake either succeeds (onopen) or the oneshot
        // sender is dropped (which only happens if the socket errored before
        // `onopen` ran).
        let open_result = open_rx
            .await
            .map_err(|_| ClientError::Transport("ws open cancelled".into()))?;
        if let Err(err) = open_result {
            clear_handlers(&ws);
            return Err(err);
        }

        Ok(Self {
            ws,
            rx,
            _on_open: on_open,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        clear_handlers(&self.ws);
    }
}

fn clear_handlers(ws: &WebSocket) {
    ws.set_onopen(None);
    ws.set_onmessage(None);
    ws.set_onclose(None);
    ws.set_onerror(None);
}

#[async_trait(?Send)]
impl Transport for WsTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> ClientResult<()> {
        // `send_with_u8_array` copies into the JS heap synchronously, so the
        // borrow of `bytes` does not need to outlive this call.
        self.ws
            .send_with_u8_array(&bytes)
            .map_err(|e| ClientError::Transport(format!("ws send: {e:?}")))
    }

    async fn recv(&mut self) -> ClientResult<Option<Vec<u8>>> {
        match self.rx.next().await {
            // A real message, or a close signal emitted by `onclose`.
            Some(Ok(opt)) => Ok(opt),
            // `onerror` fired.
            Some(Err(e)) => Err(e),
            // Channel ended without an explicit close — treat as closed.
            None => Ok(None),
        }
    }

    async fn close(&mut self) -> ClientResult<()> {
        clear_handlers(&self.ws);
        // Best-effort: the JS-side `close()` may throw if already closing.
        let _ = self.ws.close();
        Ok(())
    }
}
