use std::sync::Arc;
use std::time::Duration;

use cli_pocket_crypto::{KeyPair, NoiseAnonymousInitiator};
use cli_pocket_daemon_core::accept::{run_accepted_transport, AcceptDeps, AcceptedTransport};
use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_daemon_core::config::DaemonConfig;
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::listener::serve;
use cli_pocket_daemon_core::session::SessionManager;
use cli_pocket_proto::codec::{decode_frame, encode_frame};
use cli_pocket_proto::frame::{Frame, FrameBody};
use cli_pocket_proto::hello::{Hello, ServerInfo};
use cli_pocket_proto::{ClientId, PROTOCOL_VERSION};
use cli_pocket_transport::{TokioWsTransport, Transport};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
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
    let mut init =
        NoiseAnonymousInitiator::new(&client_keypair).expect("noise anonymous initiator");

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
        resume: None,
    }));
    send_frame(&mut transport, &mut session, &hello).await;

    let response = recv_frame(&mut transport, &mut session).await;
    assert!(matches!(response.body, FrameBody::HelloOk(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_path_negotiates_mobile_subprotocol() {
    let fixture = ListenerFixture::start().await;
    let mut request = format!("ws://{}/session", fixture.addr)
        .into_client_request()
        .expect("build websocket request");
    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        "cli-pocket-server/v1"
            .parse()
            .expect("valid subprotocol header"),
    );

    let (_, response) = connect_async(request)
        .await
        .expect("connect with mobile subprotocol");

    assert_eq!(
        response
            .headers()
            .get(SEC_WEBSOCKET_PROTOCOL)
            .and_then(|value| value.to_str().ok()),
        Some("cli-pocket-server/v1")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_path_auto_pairs_loopback_client() {
    let fixture = ListenerFixture::start().await;
    let client_keypair = KeyPair::generate().expect("client keypair");

    let mut transport = TokioWsTransport::connect(&format!("ws://{}/session", fixture.addr))
        .await
        .expect("connect session path");
    let mut init =
        NoiseAnonymousInitiator::new(&client_keypair).expect("noise anonymous initiator");

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
        resume: None,
    }));
    send_frame(&mut transport, &mut session, &hello).await;

    let response = recv_frame(&mut transport, &mut session).await;
    assert!(matches!(response.body, FrameBody::HelloOk(_)));

    let stored = fixture
        .client_db
        .lookup_by_public(&client_keypair.public)
        .await
        .expect("lookup client");
    assert!(stored.is_some(), "loopback direct client should auto-pair");
}

struct ListenerFixture {
    addr: std::net::SocketAddr,
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
        let accept_deps = AcceptDeps {
            identity: Arc::clone(&identity),
            relay_psk: None,
            config: DaemonConfig::default(),
            session_mgr,
            client_db: Arc::clone(&client_db),
            server_info: ServerInfo {
                server_version: "test".to_string(),
                server_label: None,
            },
        };
        let (accepted_tx, mut accepted_rx) =
            mpsc::channel::<AcceptedTransport<TokioWsTransport>>(8);
        let task = tokio::spawn(async move {
            let deps = accept_deps.clone();
            tokio::spawn(async move {
                while let Some(accepted) = accepted_rx.recv().await {
                    let deps = deps.clone();
                    tokio::spawn(async move {
                        let _ = run_accepted_transport(accepted, deps).await;
                    });
                }
            });
            let _ = serve(listener, accepted_tx).await;
        });

        Self {
            addr,
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
