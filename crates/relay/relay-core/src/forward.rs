use std::sync::Arc;

use bytes::Bytes;
use cli_pocket_proto::codec::{decode_relay, RelayWire};
use cli_pocket_proto::{PairCloseReason, PairId, RelayCtrl, RelayData, ServerId};
use futures_util::{SinkExt, StreamExt};
use metrics::{counter, gauge};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::caps::{Caps, ServerTicket};
use crate::pairs::Pair;
use crate::registry::{ServerMsg, ServerRegistry, ServerSlot};

const SOCKET_QUEUE_CAPACITY: usize = 32;

fn usize_gauge_value(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

pub async fn run_server_side<WS>(
    ws: WS,
    registry: ServerRegistry,
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
    let RelayWire::Ctrl(RelayCtrl::ServerRegister {
        server_id,
        server_pubkey: _,
        signature: _,
    }) = decode_relay(&first).map_err(|err| crate::codec_err_to_protocol(&err))?
    else {
        return Err(crate::RelayError::Protocol(
            "expected ServerRegister as first server frame",
        ));
    };

    let server_ticket = caps.try_add_server()?;
    let (tx, rx) = mpsc::channel(SOCKET_QUEUE_CAPACITY);
    let writer = tokio::spawn(writer_loop(sink, rx));
    let registration = register_server(&registry, server_id, tx, server_ticket)?;
    let ok_frame = crate::encode_ctrl_frame(&RelayCtrl::ServerRegisterOk);
    send_server_msg(&registration.tx(), ServerMsg::Ctrl(Bytes::from(ok_frame))).await?;

    let result = server_read_loop(stream, server_id, pairs, registration.tx()).await;

    drop(registration);
    let _ = writer.await;
    result
}

pub async fn run_client_side<WS>(
    ws: WS,
    target_server: ServerId,
    registry: ServerRegistry,
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
    let (client_tx, client_rx) = mpsc::channel(SOCKET_QUEUE_CAPACITY);
    let writer = tokio::spawn(client_writer_loop(sink, client_rx));
    let first = read_binary_frame(&mut stream).await?;
    let RelayWire::Ctrl(RelayCtrl::ClientConnect { server_id }) =
        decode_relay(&first).map_err(|err| crate::codec_err_to_protocol(&err))?
    else {
        return Err(crate::RelayError::Protocol(
            "expected ClientConnect as first client frame",
        ));
    };
    if server_id != target_server {
        return Err(crate::RelayError::Protocol(
            "client pair request server mismatch",
        ));
    }

    let server_tx = registry
        .get(&target_server)
        .ok_or(crate::RelayError::Protocol("target server not registered"))?;
    let pair_ticket = caps.try_add_pair()?;
    let pair_id = PairId(Uuid::now_v7());

    let pair = Arc::new(Pair::new(
        pair_id,
        target_server,
        pair_ticket,
        server_tx.clone(),
        client_tx.clone(),
    ));
    pairs.insert(Arc::clone(&pair));

    counter!("cli_pocket_relay_pairs_total").increment(1);
    gauge!("cli_pocket_relay_pairs_current").set(usize_gauge_value(pairs.list_for_sweep().len()));

    send_server_msg(
        &server_tx,
        ServerMsg::Ctrl(Bytes::from(crate::encode_ctrl_frame(
            &RelayCtrl::PairInbound { pair_id },
        ))),
    )
    .await?;
    send_server_msg(
        &client_tx,
        ServerMsg::Ctrl(Bytes::from(crate::encode_ctrl_frame(
            &RelayCtrl::PairOpen { pair_id },
        ))),
    )
    .await?;

    let result = client_read_loop(stream, Arc::clone(&pair), pairs.clone_handle()).await;

    close_pair(&pairs, pair_id, PairCloseReason::ClientGone).await;
    let _ = writer.await;
    result
}

async fn server_read_loop<WS>(
    mut stream: WS,
    server_id: ServerId,
    pairs: crate::pairs::PairManager,
    server_tx: mpsc::Sender<ServerMsg>,
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
                    RelayWire::Ctrl(ctrl) => {
                        if let RelayCtrl::PairClose { pair_id, reason } = ctrl {
                            close_pair(&pairs, pair_id, reason).await;
                        }
                    }
                    RelayWire::Data(RelayData::Forward { pair_id, bytes }) => {
                        let Some(pair) = pairs.get(&pair_id) else {
                            send_server_msg(
                                &server_tx,
                                ServerMsg::Ctrl(Bytes::from(crate::encode_ctrl_frame(
                                    &RelayCtrl::PairClose {
                                        pair_id,
                                        reason: PairCloseReason::ClientGone,
                                    },
                                ))),
                            )
                            .await?;
                            continue;
                        };
                        if pair.server_id != server_id {
                            return Err(crate::RelayError::Protocol("pair routed to wrong server"));
                        }
                        pair.touch();
                        counter!("cli_pocket_relay_bytes_total", "direction" => "server_to_client")
                            .increment(bytes.len() as u64);
                        send_server_msg(
                            &pair.client_tx,
                            ServerMsg::Data(Bytes::from(crate::encode_data_frame(
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

    let _ = server_tx.send(ServerMsg::Close).await;
    let affected_pairs: Vec<PairId> = pairs
        .list_for_sweep()
        .into_iter()
        .filter(|pair| pair.server_id == server_id)
        .map(|pair| pair.pair_id)
        .collect();
    for pair_id in affected_pairs {
        close_pair(&pairs, pair_id, PairCloseReason::ServerGone).await;
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
                if let Ok(RelayWire::Ctrl(RelayCtrl::PairClose { pair_id, reason })) =
                    decode_relay(&bytes)
                {
                    close_pair(&pairs, pair_id, reason).await;
                    break;
                }

                pair.touch();
                counter!("cli_pocket_relay_bytes_total", "direction" => "client_to_server")
                    .increment(bytes.len() as u64);
                send_server_msg(
                    &pair.server_tx,
                    ServerMsg::Data(Bytes::from(crate::encode_data_frame(&RelayData::Forward {
                        pair_id: pair.pair_id,
                        bytes: bytes.clone().into(),
                    }))),
                )
                .await?;
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
    mut rx: mpsc::Receiver<ServerMsg>,
) where
    WS: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::Ctrl(bytes) | ServerMsg::Data(bytes) => {
                if sink.send(Message::Binary(bytes.to_vec())).await.is_err() {
                    break;
                }
            }
            ServerMsg::Close => {
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
        }
    }
    let _ = sink.close().await;
}

async fn client_writer_loop<WS>(
    mut sink: futures_util::stream::SplitSink<WS, Message>,
    mut rx: mpsc::Receiver<ServerMsg>,
) where
    WS: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::Ctrl(bytes) => {
                if sink.send(Message::Binary(bytes.to_vec())).await.is_err() {
                    break;
                }
            }
            ServerMsg::Data(bytes) => {
                let payload = match decode_relay(&bytes) {
                    Ok(RelayWire::Data(RelayData::Forward { bytes, .. })) => bytes,
                    _ => break,
                };
                if sink.send(Message::Binary(payload.to_vec())).await.is_err() {
                    break;
                }
            }
            ServerMsg::Close => {
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
    let _ = pair.server_tx.send(ServerMsg::Ctrl(frame.clone())).await;
    let _ = pair.client_tx.send(ServerMsg::Ctrl(frame)).await;
    gauge!("cli_pocket_relay_pairs_current").set(usize_gauge_value(pairs.list_for_sweep().len()));
    counter!("cli_pocket_relay_pair_close_total", "reason" => close_reason_label(&reason))
        .increment(1);
}

fn register_server(
    registry: &ServerRegistry,
    server_id: ServerId,
    tx: mpsc::Sender<ServerMsg>,
    ticket: ServerTicket,
) -> crate::RelayResult<RegisteredServer> {
    let registration = registry.register(ServerSlot::new(server_id, tx.clone()))?;
    gauge!("cli_pocket_relay_servers_current").set(usize_gauge_value(registry.list_ids().len()));
    Ok(RegisteredServer {
        _registration: registration,
        tx,
        _ticket: ticket,
    })
}

async fn send_server_msg(tx: &mpsc::Sender<ServerMsg>, msg: ServerMsg) -> crate::RelayResult<()> {
    tx.send(msg)
        .await
        .map_err(|_| crate::RelayError::Protocol("relay peer writer closed"))
}

fn close_reason_label(reason: &PairCloseReason) -> &'static str {
    match reason {
        PairCloseReason::Normal => "normal",
        PairCloseReason::ServerGone => "server_gone",
        PairCloseReason::ClientGone => "client_gone",
        PairCloseReason::Stuck => "stuck",
        PairCloseReason::RelayShutdown => "relay_shutdown",
        PairCloseReason::Rejected(_) => "rejected",
    }
}

fn ws_err(err: &tokio_tungstenite::tungstenite::Error) -> crate::RelayError {
    crate::RelayError::Internal(format!("websocket error: {err}"))
}

struct RegisteredServer {
    _registration: crate::registry::ServerRegistration,
    tx: mpsc::Sender<ServerMsg>,
    _ticket: ServerTicket,
}

impl RegisteredServer {
    fn tx(&self) -> mpsc::Sender<ServerMsg> {
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
