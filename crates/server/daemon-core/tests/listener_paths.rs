use std::sync::Arc;
use std::time::Duration;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use cli_pocket_crypto::{KeyPair, NoiseInitiator, Spake2Side};
use cli_pocket_daemon_core::client_db::{ClientDb, ClientRecord};
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::listener::{serve, ListenerDeps};
use cli_pocket_daemon_core::pairing::PairingCodes;
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

const PAIRING_HOST_ID: &[u8] = b"cli-pocket pairing host v1";
const PAIRING_CLIENT_ID: &[u8] = b"cli-pocket pairing client v1";
const PAIRING_AEAD_NONCE: [u8; 12] = [0_u8; 12];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_path_completes_pairing_and_rotates_code() {
    let fixture = ListenerFixture::start().await;
    let before_code = fixture.pairing_codes.current_code();
    let client_keypair = KeyPair::generate().expect("client keypair");

    pair_client(
        &format!("ws://{}/pair", fixture.addr),
        &before_code,
        &client_keypair,
        &fixture.identity.public,
    )
    .await
    .expect("pair client");

    let clients = fixture.client_db.list().await;
    assert!(clients
        .iter()
        .any(|record| record.public_key == client_keypair.public));
    assert_ne!(fixture.pairing_codes.current_code(), before_code);
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
    pairing_codes: PairingCodes,
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
        let pairing_codes = PairingCodes::new(Duration::from_secs(120));
        let session_mgr = Arc::new(SessionManager::new(4));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let deps = ListenerDeps {
            identity: Arc::clone(&identity),
            psk: None,
            session_mgr,
            client_db: Arc::clone(&client_db),
            pairing_codes: pairing_codes.clone(),
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
            pairing_codes,
            _task: task,
            _dir: dir,
        }
    }
}

async fn pair_client(
    url: &str,
    code: &str,
    client_keypair: &KeyPair,
    expected_server_pk: &[u8; 32],
) -> Result<(), String> {
    let mut transport = TokioWsTransport::connect(url)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let daemon_sp_bytes = recv_transport(&mut transport).await;
    let sp = Spake2Side::start_client(code.as_bytes(), PAIRING_HOST_ID, PAIRING_CLIENT_ID);
    transport
        .send(sp.outbound().to_vec())
        .await
        .map_err(|e| format!("send spake2: {e}"))?;
    let outcome = sp
        .finish(&daemon_sp_bytes)
        .map_err(|e| format!("finish spake2: {e}"))?;

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&outcome.psk));
    let nonce = Nonce::from_slice(&PAIRING_AEAD_NONCE);
    let client_pk_ct = cipher
        .encrypt(nonce, client_keypair.public.as_ref())
        .map_err(|e| format!("encrypt client pk: {e}"))?;
    transport
        .send(client_pk_ct)
        .await
        .map_err(|e| format!("send client pk: {e}"))?;

    let server_pk_ct = recv_transport(&mut transport).await;
    let server_pk = cipher
        .decrypt(nonce, server_pk_ct.as_ref())
        .map_err(|e| format!("decrypt server pk: {e}"))?;
    assert_eq!(server_pk, expected_server_pk);
    Ok(())
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
