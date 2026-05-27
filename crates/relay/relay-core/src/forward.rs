use std::sync::Arc;

use bytes::Bytes;
use cli_pocket_proto::codec::{decode_relay, RelayWire};
use cli_pocket_proto::{HostId, PairCloseReason, PairId, RelayCtrl, RelayData};
use futures_util::{SinkExt, StreamExt};
use metrics::{counter, gauge};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::caps::{Caps, HostTicket};
use crate::pairs::Pair;
use crate::registry::{HostMsg, HostRegistry, HostSlot};

const SOCKET_QUEUE_CAPACITY: usize = 32;

fn usize_gauge_value(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

pub async fn run_host_side<WS>(
    ws: WS,
    registry: HostRegistry,
    pairs: crate::pairs::PairManager,
    caps: Caps,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    let (sink, mut stream) = ws.split();
    let first = read_binary_frame(&mut stream).await?;
    let RelayWire::Ctrl(RelayCtrl::HostRegister {
        host_id,
        host_pubkey: _,
        signature: _,
    }) = decode_relay(&first).map_err(|err| crate::codec_err_to_protocol(&err))?
    else {
        return Err(crate::RelayError::Protocol(
            "expected HostRegister as first host frame",
        ));
    };

    let host_ticket = caps.try_add_host()?;
    let (tx, rx) = mpsc::channel(SOCKET_QUEUE_CAPACITY);
    let writer = tokio::spawn(writer_loop(sink, rx));
    let registration = register_host(&registry, host_id, tx, host_ticket)?;
    let ok_frame = crate::encode_ctrl_frame(&RelayCtrl::HostRegisterOk);
    send_host_msg(&registration.tx(), HostMsg::Ctrl(Bytes::from(ok_frame))).await?;

    let result = host_read_loop(stream, host_id, pairs, registration.tx()).await;

    drop(registration);
    let _ = writer.await;
    result
}

pub async fn run_client_side<WS>(
    ws: WS,
    target_host: HostId,
    registry: HostRegistry,
    pairs: crate::pairs::PairManager,
    caps: Caps,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
        + 'static,
{
    let (sink, mut stream) = ws.split();
    let first = read_binary_frame(&mut stream).await?;
    let RelayWire::Ctrl(RelayCtrl::ClientPairRequest {
        host_id,
        attempt_token,
    }) = decode_relay(&first).map_err(|err| crate::codec_err_to_protocol(&err))?
    else {
        return Err(crate::RelayError::Protocol(
            "expected ClientPairRequest as first client frame",
        ));
    };
    if host_id != target_host {
        return Err(crate::RelayError::Protocol(
            "client pair request host mismatch",
        ));
    }

    let host_tx = registry
        .get(&target_host)
        .ok_or(crate::RelayError::Protocol("target host not registered"))?;
    let pair_ticket = caps.try_add_pair()?;
    let pair_id = PairId(Uuid::now_v7());
    let (client_tx, client_rx) = mpsc::channel(SOCKET_QUEUE_CAPACITY);
    let writer = tokio::spawn(writer_loop(sink, client_rx));

    let pair = Arc::new(Pair::new(
        pair_id,
        target_host,
        pair_ticket,
        host_tx.clone(),
        client_tx.clone(),
    ));
    pairs.insert(Arc::clone(&pair));

    counter!("cli_pocket_relay_pairs_total").increment(1);
    gauge!("cli_pocket_relay_pairs_current").set(usize_gauge_value(pairs.list_for_sweep().len()));

    send_host_msg(
        &host_tx,
        HostMsg::Ctrl(Bytes::from(crate::encode_ctrl_frame(
            &RelayCtrl::PairInbound {
                pair_id,
                attempt_token,
            },
        ))),
    )
    .await?;
    send_host_msg(
        &client_tx,
        HostMsg::Ctrl(Bytes::from(crate::encode_ctrl_frame(
            &RelayCtrl::PairOpen { pair_id },
        ))),
    )
    .await?;

    let result = client_read_loop(stream, Arc::clone(&pair), pairs.clone_handle()).await;

    close_pair(&pairs, pair_id, PairCloseReason::ClientGone).await;
    let _ = writer.await;
    result
}

async fn host_read_loop<WS>(
    mut stream: WS,
    host_id: HostId,
    pairs: crate::pairs::PairManager,
    host_tx: mpsc::Sender<HostMsg>,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin
        + Send
        + 'static,
{
    while let Some(msg) = stream.next().await {
        match msg.map_err(|err| ws_err(&err))? {
            Message::Binary(bytes) => {
                match decode_relay(&bytes).map_err(|err| crate::codec_err_to_protocol(&err))? {
                    RelayWire::Ctrl(ctrl) => match ctrl {
                        RelayCtrl::HostUnregister => break,
                        RelayCtrl::PairClose { pair_id, reason } => {
                            close_pair(&pairs, pair_id, reason).await;
                        }
                        _ => {}
                    },
                    RelayWire::Data(RelayData::Forward { pair_id, bytes }) => {
                        let pair = pairs
                            .get(&pair_id)
                            .ok_or(crate::RelayError::Protocol("unknown pair id"))?;
                        if pair.host_id != host_id {
                            return Err(crate::RelayError::Protocol("pair routed to wrong host"));
                        }
                        pair.touch();
                        counter!("cli_pocket_relay_bytes_total", "direction" => "host_to_client")
                            .increment(bytes.len() as u64);
                        send_host_msg(
                            &pair.client_tx,
                            HostMsg::Data(Bytes::from(crate::encode_data_frame(
                                &RelayData::Forward { pair_id, bytes },
                            ))),
                        )
                        .await?;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Text(_) | Message::Frame(_) => {
                return Err(crate::RelayError::Protocol(
                    "unexpected non-binary frame on relay socket",
                ));
            }
        }
    }

    let _ = host_tx.send(HostMsg::Close).await;
    let affected_pairs: Vec<PairId> = pairs
        .list_for_sweep()
        .into_iter()
        .filter(|pair| pair.host_id == host_id)
        .map(|pair| pair.pair_id)
        .collect();
    for pair_id in affected_pairs {
        close_pair(&pairs, pair_id, PairCloseReason::HostGone).await;
    }
    Ok(())
}

async fn client_read_loop<WS>(
    mut stream: WS,
    pair: Arc<Pair>,
    pairs: crate::pairs::PairManager,
) -> crate::RelayResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin
        + Send
        + 'static,
{
    while let Some(msg) = stream.next().await {
        match msg.map_err(|err| ws_err(&err))? {
            Message::Binary(bytes) => {
                match decode_relay(&bytes).map_err(|err| crate::codec_err_to_protocol(&err))? {
                    RelayWire::Data(RelayData::Forward { pair_id, bytes }) => {
                        if pair_id != pair.pair_id {
                            return Err(crate::RelayError::Protocol(
                                "client forwarded wrong pair id",
                            ));
                        }
                        pair.touch();
                        counter!("cli_pocket_relay_bytes_total", "direction" => "client_to_host")
                            .increment(bytes.len() as u64);
                        send_host_msg(
                            &pair.host_tx,
                            HostMsg::Data(Bytes::from(crate::encode_data_frame(
                                &RelayData::Forward { pair_id, bytes },
                            ))),
                        )
                        .await?;
                    }
                    RelayWire::Ctrl(RelayCtrl::PairClose { pair_id, reason }) => {
                        close_pair(&pairs, pair_id, reason).await;
                        break;
                    }
                    RelayWire::Ctrl(_) => {}
                }
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Text(_) | Message::Frame(_) => {
                return Err(crate::RelayError::Protocol(
                    "unexpected non-binary frame on relay socket",
                ));
            }
        }
    }

    Ok(())
}

async fn writer_loop<WS>(
    mut sink: futures_util::stream::SplitSink<WS, Message>,
    mut rx: mpsc::Receiver<HostMsg>,
) where
    WS: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    while let Some(msg) = rx.recv().await {
        match msg {
            HostMsg::Ctrl(bytes) | HostMsg::Data(bytes) => {
                if sink.send(Message::Binary(bytes.to_vec())).await.is_err() {
                    break;
                }
            }
            HostMsg::Close => {
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
        }
    }
    let _ = sink.close().await;
}

async fn read_binary_frame<WS>(stream: &mut WS) -> crate::RelayResult<Vec<u8>>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = stream
            .next()
            .await
            .ok_or(crate::RelayError::Protocol("relay websocket closed"))?
            .map_err(|err| ws_err(&err))?;
        match msg {
            Message::Binary(bytes) => return Ok(bytes.clone()),
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => return Err(crate::RelayError::Protocol("relay websocket closed")),
            Message::Text(_) | Message::Frame(_) => {
                return Err(crate::RelayError::Protocol(
                    "unexpected non-binary frame on relay socket",
                ));
            }
        }
    }
}

async fn close_pair(pairs: &crate::pairs::PairManager, pair_id: PairId, reason: PairCloseReason) {
    let Some(pair) = pairs.remove(&pair_id) else {
        return;
    };

    let frame = Bytes::from(crate::encode_ctrl_frame(&RelayCtrl::PairClose {
        pair_id,
        reason: reason.clone(),
    }));
    let _ = pair.host_tx.send(HostMsg::Ctrl(frame.clone())).await;
    let _ = pair.client_tx.send(HostMsg::Ctrl(frame)).await;
    gauge!("cli_pocket_relay_pairs_current").set(usize_gauge_value(pairs.list_for_sweep().len()));
    counter!("cli_pocket_relay_pair_close_total", "reason" => close_reason_label(&reason))
        .increment(1);
}

fn register_host(
    registry: &HostRegistry,
    host_id: HostId,
    tx: mpsc::Sender<HostMsg>,
    ticket: HostTicket,
) -> crate::RelayResult<RegisteredHost> {
    let registration = registry.register(HostSlot::new(host_id, tx.clone()))?;
    gauge!("cli_pocket_relay_hosts_current").set(usize_gauge_value(registry.list_ids().len()));
    Ok(RegisteredHost {
        _registration: registration,
        tx,
        _ticket: ticket,
    })
}

async fn send_host_msg(tx: &mpsc::Sender<HostMsg>, msg: HostMsg) -> crate::RelayResult<()> {
    tx.send(msg)
        .await
        .map_err(|_| crate::RelayError::Protocol("relay peer writer closed"))
}

fn close_reason_label(reason: &PairCloseReason) -> &'static str {
    match reason {
        PairCloseReason::Normal => "normal",
        PairCloseReason::HostGone => "host_gone",
        PairCloseReason::ClientGone => "client_gone",
        PairCloseReason::Stuck => "stuck",
        PairCloseReason::RelayShutdown => "relay_shutdown",
        PairCloseReason::Rejected(_) => "rejected",
    }
}

fn ws_err(err: &tokio_tungstenite::tungstenite::Error) -> crate::RelayError {
    crate::RelayError::Internal(format!("websocket error: {err}"))
}

struct RegisteredHost {
    _registration: crate::registry::HostRegistration,
    tx: mpsc::Sender<HostMsg>,
    _ticket: HostTicket,
}

impl RegisteredHost {
    fn tx(&self) -> mpsc::Sender<HostMsg> {
        self.tx.clone()
    }
}

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
