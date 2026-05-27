use std::sync::Arc;
use std::time::Duration;

use cli_pocket_crypto::{KeyPair, NoiseInitiator};
use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::listener::{serve, ListenerDeps};
use cli_pocket_daemon_core::session::SessionManager;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::{Capabilities, ClientKind, Hello, ServerInfo};
use cli_pocket_proto::{ClientId, PROTOCOL_VERSION};
use cli_pocket_transport::{TokioWsTransport, Transport};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_path_is_rejected() {
    let fixture = ListenerFixture::start().await;
    let mut transport = TokioWsTransport::connect(&format!("ws://{}/pair", fixture.addr))
        .await
        .expect("websocket upgrade succeeds before path validation");
    let first = timeout(Duration::from_secs(5), transport.recv()).await;

    assert!(
        matches!(first, Ok(Ok(None) | Err(_))),
        "/pair should become unusable immediately because direct pairing is removed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_path_accepts_paired_client() {
    let fixture = ListenerFixture::start().await;
    let client_keypair = KeyPair::generate().expect("client keypair");
    fixture
        .client_db
        .add(ClientRecord {
            client_id: ClientId(Uuid::from_bytes([0x42; 16])),
            public_key: client_keypair.public,
            paired_at: 0,
        })
        .await
        .expect("add client");

    let mut transport = TokioWsTransport::connect(&format!("ws://{}/session", fixture.addr))
        .await
        .expect("connect session path");
    let mut init = NoiseInitiator::new(&client_keypair, &fixture.identity.public, None)
        .expect("noise initiator");

    let msg1 = init.write_handshake().expect("write msg1");
    transport.send(msg1).await.expect("send msg1");
    let msg2 = recv_transport(&mut transport).await;
    init.read_handshake(&msg2).expect("read msg2");
    let msg3 = init.write_handshake().expect("write msg3");
    transport.send(msg3).await.expect("send msg3");
    let mut session = init.finish().expect("finish noise");

    let hello = Frame::body(FrameBody::Hello(Hello {
        protocol_min: PROTOCOL_VERSION,
        protocol_max: PROTOCOL_VERSION,
        capabilities: Capabilities::NONE,
        client_kind: ClientKind::Cli,
        resume: None,
    }));
    send_frame(&mut transport, &mut session, &hello).await;

    let response = recv_frame(&mut transport, &mut session).await;
    assert!(matches!(response.body, FrameBody::HelloOk(_)));
}

struct ListenerFixture {
    addr: std::net::SocketAddr,
    identity: Arc<KeyPair>,
    client_db: Arc<ClientDb>,
    _task: tokio::task::JoinHandle<()>,
    _dir: TempDir,
}

impl ListenerFixture {
    async fn start() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let identity = Arc::new(
            load_or_create(&dir.path().join("identity.json"))
                .expect("identity")
                .keypair,
        );
        let client_db = Arc::new(
            ClientDb::open(
                &dir.path().join("clients.json"),
                &dir.path().join("revoked.json"),
            )
            .await
            .expect("client db"),
        );
        let session_mgr = Arc::new(SessionManager::new(4));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let deps = ListenerDeps {
            identity: Arc::clone(&identity),
            psk: None,
            session_mgr,
            client_db: Arc::clone(&client_db),
            server_info: ServerInfo {
                server_version: "test".to_string(),
                host_label: None,
            },
        };
        let task = tokio::spawn(async move {
            let _ = serve(listener, deps).await;
        });

        Self {
            addr,
            identity,
            client_db,
            _task: task,
            _dir: dir,
        }
    }
}

async fn recv_transport(transport: &mut TokioWsTransport) -> Vec<u8> {
    timeout(Duration::from_secs(5), transport.recv())
        .await
        .expect("transport recv timed out")
        .expect("transport recv error")
        .expect("transport closed")
}

async fn send_frame(
    transport: &mut TokioWsTransport,
    session: &mut cli_pocket_crypto::NoiseSession,
    frame: &Frame,
) {
    let plain = encode_frame(frame).expect("encode frame");
    let ct = session.encrypt(&plain).expect("encrypt frame");
    transport.send(ct).await.expect("send frame");
}

async fn recv_frame(
    transport: &mut TokioWsTransport,
    session: &mut cli_pocket_crypto::NoiseSession,
) -> Frame {
    let ct = recv_transport(transport).await;
    let plain = session.decrypt(&ct).expect("decrypt frame");
    decode_frame(&plain).expect("decode frame")
}
