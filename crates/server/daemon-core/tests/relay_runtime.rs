use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use cli_pocket_crypto::{KeyPair, NoiseInitiator};
use cli_pocket_daemon_core::client_db::ClientRecord;
use cli_pocket_daemon_core::config::{
    AppConfig, DaemonConfig, LimitsConfig, ListenConfig, RelayConfig, SecurityConfig,
};
use cli_pocket_daemon_core::server::Daemon;
use cli_pocket_proto::codec::{
    decode_frame, decode_relay, encode_frame, encode_relay_ctrl, RelayWire,
};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::Hello;
use cli_pocket_proto::{ClientId, RelayCtrl, ServerId, PROTOCOL_VERSION};
use cli_pocket_relay_core::http::router;
use cli_pocket_relay_core::{RelayConfig as RelayServerConfig, RelayServer};
use futures_util::{SinkExt, StreamExt};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_registers_with_relay() {
    let relay = RelayFixture::start().await;
    let mut daemon = DaemonFixture::boot(relay.addr).await;
    let server_id = daemon.server_id();

    daemon.start().await.expect("start daemon");

    let registered = timeout(Duration::from_secs(5), async {
        loop {
            if relay.server.state.registry.get(&server_id).is_some() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await;

    assert!(
        registered.is_ok(),
        "daemon server id should appear in relay registry"
    );

    daemon.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_transport_reaches_daemon_handshake() {
    let relay = RelayFixture::start().await;
    let mut daemon = DaemonFixture::boot(relay.addr).await;
    let server_id = daemon.server_id();
    let client_keypair = KeyPair::generate().expect("client keypair");

    daemon.add_paired_client(&client_keypair).await;
    daemon.start().await.expect("start daemon");
    daemon.wait_until_registered(&relay, server_id).await;

    let (mut client_ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{}/ws/client?server={}",
        relay.addr, server_id.0
    ))
    .await
    .expect("connect client relay socket");
    client_ws
        .send(Message::Binary(
            encode_relay_ctrl(&RelayCtrl::ClientConnect { server_id })
                .expect("encode pair request"),
        ))
        .await
        .expect("send pair request");

    match recv_relay(&mut client_ws).await {
        RelayWire::Ctrl(RelayCtrl::PairOpen { .. }) => {}
        other => panic!("expected PairOpen, got {other:?}"),
    }

    let mut initiator =
        NoiseInitiator::new(&client_keypair, &daemon.public_key(), None).expect("initiator");
    let msg1 = initiator.write_handshake().expect("write msg1");
    client_ws
        .send(Message::Binary(msg1))
        .await
        .expect("send handshake msg1");

    let msg2 = recv_client_bytes(&mut client_ws).await;
    initiator.read_handshake(&msg2).expect("read msg2");

    let msg3 = initiator.write_handshake().expect("write msg3");
    client_ws
        .send(Message::Binary(msg3))
        .await
        .expect("send handshake msg3");

    let mut session = initiator.finish().expect("finish initiator");
    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        resume: None,
    }));
    let hello_bytes = encode_frame(&hello).expect("encode hello");
    let hello_ciphertext = session.encrypt(&hello_bytes).expect("encrypt hello");
    client_ws
        .send(Message::Binary(hello_ciphertext))
        .await
        .expect("send hello");

    let hello_response = recv_client_bytes(&mut client_ws).await;
    let plaintext = session
        .decrypt(&hello_response)
        .expect("decrypt hello response");
    let response = decode_frame(&plaintext).expect("decode hello response");
    assert!(matches!(response.body, FrameBody::HelloOk(_)));

    daemon.shutdown().await;
}

struct RelayFixture {
    addr: SocketAddr,
    server: RelayServer,
    _task: tokio::task::JoinHandle<()>,
}

impl RelayFixture {
    async fn start() -> Self {
        let mut config = RelayServerConfig::default();
        config.listen.addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        config.listen.port = 0;
        let server = RelayServer::new(config);
        let app = router(server.state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
        let addr = listener.local_addr().expect("relay addr");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            addr,
            server,
            _task: task,
        }
    }
}

struct DaemonFixture {
    daemon: Daemon,
    _dir: TempDir,
}

impl DaemonFixture {
    async fn boot(relay_addr: SocketAddr) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let config = DaemonConfig {
            listen: ListenConfig {
                addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 0,
            },
            security: SecurityConfig {
                identity_path: dir.path().join("identity.json"),
                clients_path: dir.path().join("clients.json"),
                revoked_path: dir.path().join("revoked.json"),
            },
            relay: RelayConfig {
                base_url: format!("ws://{relay_addr}"),
                psk_hex: String::new(),
                server_auth_token: None,
            },
            app: AppConfig::default(),
            limits: LimitsConfig::default(),
        };
        let daemon = Daemon::boot(config).await.expect("boot daemon");
        Self { daemon, _dir: dir }
    }

    fn server_id(&self) -> ServerId {
        self.daemon.identity.server_id
    }

    fn public_key(&self) -> [u8; 32] {
        self.daemon.identity.keypair.public
    }

    async fn add_paired_client(&self, client_keypair: &KeyPair) {
        self.daemon
            .client_db
            .add(ClientRecord {
                client_id: ClientId(Uuid::from_bytes([0x33; 16])),
                public_key: client_keypair.public,
                paired_at: 0,
            })
            .await
            .expect("add paired client");
    }

    async fn start(&mut self) -> cli_pocket_daemon_core::DaemonResult<()> {
        self.daemon.start().await
    }

    async fn wait_until_registered(&self, relay: &RelayFixture, server_id: ServerId) {
        timeout(Duration::from_secs(5), async {
            loop {
                if relay.server.state.registry.get(&server_id).is_some() {
                    break;
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("daemon should register with relay");
    }
    async fn shutdown(self) {
        self.daemon.shutdown().await;
    }
}

async fn recv_relay<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> RelayWire
where
    tokio_tungstenite::WebSocketStream<S>:
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

async fn recv_client_bytes<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> Vec<u8>
where
    tokio_tungstenite::WebSocketStream<S>:
        StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for client bytes")
        .expect("websocket closed")
        .expect("websocket frame");
    let Message::Binary(bytes) = frame else {
        panic!("expected binary client bytes, got {frame:?}");
    };
    bytes.clone()
}
