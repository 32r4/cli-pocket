use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use cli_pocket_proto::codec::{decode_relay, encode_relay_ctrl, encode_relay_data, RelayWire};
use cli_pocket_proto::{HostId, RelayCtrl, RelayData};
use cli_pocket_relay_core::http::router;
use cli_pocket_relay_core::{RelayConfig, RelayServer};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;

#[tokio::test(flavor = "current_thread")]
async fn host_and_client_can_open_one_pair_and_forward_bytes() {
    let (addr, _handle) = start_relay().await;
    let host_id = HostId(Uuid::now_v7());

    let (mut host_ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws/host"))
        .await
        .expect("connect host websocket");
    host_ws
        .send(Message::Binary(
            encode_relay_ctrl(&RelayCtrl::HostRegister {
                host_id,
                host_pubkey: vec![1, 2, 3].into(),
                signature: vec![4, 5, 6].into(),
            })
            .expect("encode host register"),
        ))
        .await
        .expect("send host register");

    assert!(matches!(
        recv_relay(&mut host_ws).await,
        RelayWire::Ctrl(RelayCtrl::HostRegisterOk)
    ));

    let (mut client_ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws/client?host={}", host_id.0))
            .await
            .expect("connect client websocket");
    client_ws
        .send(Message::Binary(
            encode_relay_ctrl(&RelayCtrl::ClientPairRequest {
                host_id,
                attempt_token: 7,
            })
            .expect("encode pair request"),
        ))
        .await
        .expect("send pair request");

    let pair_id = match recv_relay(&mut host_ws).await {
        RelayWire::Ctrl(RelayCtrl::PairInbound {
            pair_id,
            attempt_token,
        }) => {
            assert_eq!(attempt_token, 7);
            pair_id
        }
        other => panic!("expected PairInbound, got {other:?}"),
    };

    match recv_relay(&mut client_ws).await {
        RelayWire::Ctrl(RelayCtrl::PairOpen { pair_id: opened }) => {
            assert_eq!(opened, pair_id);
        }
        other => panic!("expected PairOpen, got {other:?}"),
    }

    let client_bytes = b"client->host".to_vec();
    client_ws
        .send(Message::Binary(
            encode_relay_data(&RelayData::Forward {
                pair_id,
                bytes: client_bytes.clone().into(),
            })
            .expect("encode client forward"),
        ))
        .await
        .expect("send client forward");

    match recv_relay(&mut host_ws).await {
        RelayWire::Data(RelayData::Forward {
            pair_id: forwarded_pair,
            bytes,
        }) => {
            assert_eq!(forwarded_pair, pair_id);
            assert_eq!(bytes.as_ref(), client_bytes.as_slice());
        }
        other => panic!("expected forwarded host data, got {other:?}"),
    }

    let host_bytes = b"host->client".to_vec();
    host_ws
        .send(Message::Binary(
            encode_relay_data(&RelayData::Forward {
                pair_id,
                bytes: host_bytes.clone().into(),
            })
            .expect("encode host forward"),
        ))
        .await
        .expect("send host forward");

    match recv_relay(&mut client_ws).await {
        RelayWire::Data(RelayData::Forward {
            pair_id: forwarded_pair,
            bytes,
        }) => {
            assert_eq!(forwarded_pair, pair_id);
            assert_eq!(bytes.as_ref(), host_bytes.as_slice());
        }
        other => panic!("expected forwarded client data, got {other:?}"),
    }
}

async fn start_relay() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut config = RelayConfig::default();
    config.listen.addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    config.listen.port = 0;

    let server = RelayServer::new(config);
    let app = router(server.state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
    let addr = listener.local_addr().expect("relay local addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

async fn recv_relay<S>(ws: &mut WebSocketStream<S>) -> RelayWire
where
    WebSocketStream<S>:
        StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for relay frame")
        .expect("websocket closed")
        .expect("websocket frame");
    let Message::Binary(bytes) = frame else {
        panic!("expected binary relay frame, got {frame:?}");
    };
    decode_relay(&bytes).expect("decode relay frame")
}
