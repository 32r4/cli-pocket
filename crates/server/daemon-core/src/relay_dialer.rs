use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cli_pocket_crypto::KeyPair;
use cli_pocket_proto::codec::{decode_relay, encode_relay_ctrl, encode_relay_data, RelayWire};
use cli_pocket_proto::{HostId, PairId, RelayCtrl, RelayData};
use cli_pocket_transport::{Transport, TransportError};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tracing::info;

use crate::accept::{AcceptedTransport, AcceptedTransportKind};
use crate::config::RelayConfig;

const PAIR_QUEUE_CAPACITY: usize = 32;

#[derive(Debug)]
pub struct PairTransport {
    to_daemon_rx: mpsc::Receiver<Vec<u8>>,
    from_daemon_tx: mpsc::Sender<Vec<u8>>,
}

impl PairTransport {
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, crate::DaemonError> {
        Ok(self.to_daemon_rx.recv().await)
    }

    pub async fn send(&mut self, bytes: Vec<u8>) -> Result<(), crate::DaemonError> {
        self.from_daemon_tx
            .send(bytes)
            .await
            .map_err(|_| crate::DaemonError::Internal("relay pair bridge closed".into()))
    }
}

#[async_trait]
impl Transport for PairTransport {
    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.from_daemon_tx
            .send(bytes)
            .await
            .map_err(|_| TransportError::Closed)
    }

    async fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        Ok(self.to_daemon_rx.recv().await)
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.to_daemon_rx.close();
        Ok(())
    }
}

struct PairBridge {
    to_daemon_tx: mpsc::Sender<Vec<u8>>,
}

pub async fn run(
    config: RelayConfig,
    host_id: HostId,
    identity: KeyPair,
    accepted_tx: mpsc::Sender<AcceptedTransport<PairTransport>>,
) -> crate::DaemonResult<()> {
    info!(
        url = %config.url,
        host_id = ?host_id,
        host_token = config.host_token.is_some(),
        psk_len = config.psk_hex.len(),
        "relay dialer starting"
    );

    let (ws, _) = tokio_tungstenite::connect_async(&config.url)
        .await
        .map_err(|e| crate::DaemonError::Internal(format!("relay connect failed: {e}")))?;
    let (sink, mut stream) = ws.split();
    let sink = Arc::new(Mutex::new(sink));

    sink.lock()
        .await
        .send(Message::Binary(
            encode_relay_ctrl(&RelayCtrl::HostRegister {
                host_id,
                host_pubkey: identity.public.to_vec().into(),
                signature: Vec::new().into(),
            })
            .map_err(|e| crate::DaemonError::Internal(format!("encode register failed: {e}")))?,
        ))
        .await
        .map_err(|e| crate::DaemonError::Internal(format!("send register failed: {e}")))?;

    let first = next_binary(&mut stream).await?;
    match decode_relay(&first).map_err(|err| codec_err(&err))? {
        RelayWire::Ctrl(RelayCtrl::HostRegisterOk) => {}
        RelayWire::Ctrl(RelayCtrl::HostRegisterErr { reason }) => {
            return Err(crate::DaemonError::Internal(format!(
                "relay rejected host registration: {reason}"
            )));
        }
        other => {
            return Err(crate::DaemonError::Internal(format!(
                "unexpected relay registration response: {other:?}"
            )));
        }
    }

    let heartbeat_sink = Arc::clone(&sink);
    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let frame = match encode_relay_ctrl(&RelayCtrl::HostHeartbeat) {
                Ok(frame) => frame,
                Err(_) => break,
            };
            if heartbeat_sink
                .lock()
                .await
                .send(Message::Binary(frame))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let pairs: Arc<Mutex<HashMap<PairId, PairBridge>>> = Arc::new(Mutex::new(HashMap::new()));
    let result = read_loop(stream, Arc::clone(&sink), pairs, accepted_tx).await;
    heartbeat.abort();
    result
}

async fn read_loop<WS>(
    mut stream: WS,
    sink: Arc<
        Mutex<
            futures_util::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                Message,
            >,
        >,
    >,
    pairs: Arc<Mutex<HashMap<PairId, PairBridge>>>,
    accepted_tx: mpsc::Sender<AcceptedTransport<PairTransport>>,
) -> crate::DaemonResult<()>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(msg) = stream.next().await {
        match msg.map_err(|err| ws_err(&err))? {
            Message::Binary(bytes) => match decode_relay(&bytes).map_err(|err| codec_err(&err))? {
                RelayWire::Ctrl(RelayCtrl::PairInbound { pair_id }) => {
                    let (to_daemon_tx, to_daemon_rx) =
                        mpsc::channel::<Vec<u8>>(PAIR_QUEUE_CAPACITY);
                    let (from_daemon_tx, mut from_daemon_rx) =
                        mpsc::channel::<Vec<u8>>(PAIR_QUEUE_CAPACITY);
                    pairs
                        .lock()
                        .await
                        .insert(pair_id, PairBridge { to_daemon_tx });

                    let writer = Arc::clone(&sink);
                    tokio::spawn(async move {
                        while let Some(bytes) = from_daemon_rx.recv().await {
                            let frame = match encode_relay_data(&RelayData::Forward {
                                pair_id,
                                bytes: bytes.into(),
                            }) {
                                Ok(frame) => frame,
                                Err(_) => break,
                            };
                            if writer
                                .lock()
                                .await
                                .send(Message::Binary(frame))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    });

                    accepted_tx
                        .send(AcceptedTransport {
                            label: format!("relay:{pair_id:?}"),
                            kind: AcceptedTransportKind::Relay,
                            transport: PairTransport {
                                to_daemon_rx,
                                from_daemon_tx,
                            },
                        })
                        .await
                        .map_err(|_| {
                            crate::DaemonError::Internal("relay inbound consumer dropped".into())
                        })?;
                }
                RelayWire::Data(RelayData::Forward { pair_id, bytes }) => {
                    let tx = pairs
                        .lock()
                        .await
                        .get(&pair_id)
                        .map(|bridge| bridge.to_daemon_tx.clone())
                        .ok_or_else(|| {
                            crate::DaemonError::Internal(format!(
                                "received relay bytes for unknown pair {pair_id:?}"
                            ))
                        })?;
                    tx.send(bytes.to_vec()).await.map_err(|_| {
                        crate::DaemonError::Internal("relay pair receiver dropped".into())
                    })?;
                }
                RelayWire::Ctrl(RelayCtrl::PairClose { pair_id, .. }) => {
                    pairs.lock().await.remove(&pair_id);
                }
                RelayWire::Ctrl(_) => {}
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Text(_) | Message::Frame(_) => {
                return Err(crate::DaemonError::Internal(
                    "unexpected non-binary frame on relay dialer".into(),
                ));
            }
        }
    }

    Ok(())
}

async fn next_binary<WS>(stream: &mut WS) -> crate::DaemonResult<Vec<u8>>
where
    WS: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = stream
            .next()
            .await
            .ok_or_else(|| crate::DaemonError::Internal("relay websocket closed".into()))?
            .map_err(|err| ws_err(&err))?;
        match msg {
            Message::Binary(bytes) => return Ok(bytes.clone()),
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => {
                return Err(crate::DaemonError::Internal(
                    "relay websocket closed".into(),
                ))
            }
            Message::Text(_) | Message::Frame(_) => {
                return Err(crate::DaemonError::Internal(
                    "unexpected non-binary frame on relay dialer".into(),
                ))
            }
        }
    }
}

fn codec_err(err: &cli_pocket_proto::CodecError) -> crate::DaemonError {
    crate::DaemonError::Internal(format!("relay codec error: {err}"))
}

fn ws_err(err: &tokio_tungstenite::tungstenite::Error) -> crate::DaemonError {
    crate::DaemonError::Internal(format!("relay websocket error: {err}"))
}
