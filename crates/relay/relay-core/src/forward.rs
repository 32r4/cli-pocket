//! Ciphertext forwarding. Plan E5 skeleton.
//!
//! Each WebSocket connection runs as one read task + one write task. The read
//! task parses the discriminator byte at the head of every binary frame and
//! routes the payload:
//!
//! - `RELAY_DISC_CTRL` -> decode `RelayCtrl`, handle inline (e.g. open / close
//!   a pair, register / unregister a host).
//! - `RELAY_DISC_DATA` -> decode `RelayData`, look up the pair in
//!   [`PairManager`], and forward the bytes onto the *other* side's
//!   `mpsc::Sender<PairMsg>`.
//!
//! The write task drains its `mpsc::Receiver` and emits WS binary frames.
//!
//! The two driver functions below are intentionally skeletons — they capture
//! the call-site shape (parameters and lifetimes) so callers in Tasks E7+ can
//! wire them up, while the concrete split-stream plumbing is implemented in a
//! later task once the server facade exists. Their bodies are `todo!()` so
//! `cargo check` succeeds without requiring the runtime wiring.

use cli_pocket_proto::HostId;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::caps::Caps;
use crate::pairs::PairManager;
use crate::registry::{HostMsg, HostRegistry};

/// Drive the host-side forwarder.
///
/// `rx` is the receiver paired with the [`HostRegistry`] slot that the host
/// has just registered under — every `HostMsg` the relay wants written to
/// this host's WebSocket arrives through it.
///
/// Implementation outline (filled in by Task E7+):
///
/// 1. `let (sink, stream) = ws.split();`
/// 2. Spawn a writer task that pulls `HostMsg::{Ctrl, Data}` from `rx`,
///    emits `Message::Binary(bytes)`, and exits on `HostMsg::Close`.
/// 3. In this task, consume the read half. For each binary message call
///    [`split_disc`] to peel the leading byte, decode either `RelayCtrl`
///    (handled inline against `registry` / `pairs` / `caps`) or `RelayData`
///    (route via `pairs.get(pair_id).client_tx.send(PairMsg::HostToClient(..))`).
#[allow(clippy::unused_async)] // async signature required for Task E7 wiring
pub async fn run_host_side<WS>(
    mut ws: WS,
    host_id: HostId,
    rx: mpsc::Receiver<HostMsg>,
    registry: HostRegistry,
    pairs: PairManager,
    caps: Caps,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    // The full split-stream wiring lands in a follow-up task. For now we
    // cleanly close the WebSocket so a connection that reaches this stub does
    // not leak; the `_` bindings document the surface the follow-up will use.
    let _ = (host_id, rx, registry, pairs, caps);
    tracing::warn!("host-side forwarder reached stub; closing socket");
    let _ = SinkExt::send(&mut ws, Message::Close(None)).await;
    let _ = SinkExt::close(&mut ws).await;
    Ok(())
}

/// Drive the client-side forwarder.
///
/// The client's first frame is a `RelayCtrl::ClientPairRequest { host_id }`.
/// The forwarder looks `host_id` up in `registry`, mints a `PairId`,
/// allocates a [`crate::caps::PairTicket`] (returning an error frame and
/// closing the socket if any of those steps fail), builds two bounded
/// `mpsc::channel`s, wires a `Pair{}` into `pairs`, and proxies subsequent
/// `RelayData::Forward` frames onto the host side via the per-pair sender.
#[allow(clippy::unused_async)] // async signature required for Task E7 wiring
pub async fn run_client_side<WS>(
    mut ws: WS,
    target_host: HostId,
    registry: HostRegistry,
    pairs: PairManager,
    caps: Caps,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    let _ = (target_host, registry, pairs, caps);
    tracing::warn!("client-side forwarder reached stub; closing socket");
    let _ = SinkExt::send(&mut ws, Message::Close(None)).await;
    let _ = SinkExt::close(&mut ws).await;
    Ok(())
}

/// Peel the leading discriminator byte from a binary WS frame and return
/// `(disc, rest)`. The empty-frame case is mapped to `RelayError::Protocol`
/// so callers can drop the connection without taking down the server.
pub fn split_disc(msg: &[u8]) -> crate::RelayResult<(u8, &[u8])> {
    let (disc, rest) = msg
        .split_first()
        .ok_or(crate::RelayError::Protocol("empty relay frame"))?;
    Ok((*disc, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_pocket_proto::{RELAY_DISC_CTRL, RELAY_DISC_DATA};

    #[test]
    fn split_disc_extracts_leading_byte() {
        let frame = [RELAY_DISC_DATA, 1, 2, 3];
        let (disc, rest) = split_disc(&frame).expect("non-empty frame");
        assert_eq!(disc, RELAY_DISC_DATA);
        assert_eq!(rest, &[1, 2, 3]);
    }

    #[test]
    fn split_disc_accepts_empty_payload_after_discriminator() {
        let frame = [RELAY_DISC_CTRL];
        let (disc, rest) = split_disc(&frame).expect("single-byte frame");
        assert_eq!(disc, RELAY_DISC_CTRL);
        assert!(rest.is_empty());
    }

    #[test]
    fn split_disc_rejects_empty_frame() {
        let err = split_disc(&[]).expect_err("empty frame must error");
        assert!(matches!(err, crate::RelayError::Protocol(_)));
    }
}
