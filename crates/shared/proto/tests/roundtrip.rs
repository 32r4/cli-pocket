use cli_pocket_proto::*;
use proptest::prelude::*;
use serde_bytes::ByteBuf;
use uuid::Uuid;

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<[u8; 16]>().prop_map(Uuid::from_bytes)
}

fn arb_bytes(max_len: usize) -> impl Strategy<Value = ByteBuf> {
    prop::collection::vec(any::<u8>(), 0..=max_len).prop_map(ByteBuf::from)
}

fn arb_string(max_len: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..=max_len).prop_map(|bytes| {
        bytes
            .into_iter()
            .map(|b| char::from(b'a' + (b % 26)))
            .collect()
    })
}

fn arb_terminal_create_params() -> impl Strategy<Value = TerminalCreateParams> {
    (
        1u16..=240,
        1u16..=120,
        prop::option::of(arb_string(32)),
        prop::collection::vec(arb_string(8), 0..=4),
        prop::collection::vec((arb_string(8), arb_string(16)), 0..=4),
    )
        .prop_map(|(cols, rows, cwd, cmd, env)| TerminalCreateParams {
            cols,
            rows,
            cwd,
            cmd,
            env,
        })
}

fn arb_terminal_info() -> impl Strategy<Value = TerminalInfo> {
    (
        arb_uuid(),
        any::<u16>(),
        any::<u16>(),
        any::<u64>(),
        prop::option::of(arb_string(32)),
        any::<u32>(),
    )
        .prop_map(
            |(terminal, cols, rows, created_at_unix_ms, label, attached_clients)| TerminalInfo {
                terminal: TerminalId(terminal),
                cols,
                rows,
                created_at_unix_ms,
                label,
                attached_clients,
            },
        )
}

fn arb_server_config() -> impl Strategy<Value = ServerConfig> {
    any::<u32>().prop_map(|scrollback_bytes| ServerConfig { scrollback_bytes })
}

fn arb_request_body() -> impl Strategy<Value = RequestBody> {
    prop_oneof![
        Just(RequestBody::ListTerminals),
        arb_terminal_create_params().prop_map(|params| RequestBody::CreateTerminal { params }),
        arb_uuid().prop_map(|terminal_id| RequestBody::OpenTerminal {
            terminal_id: TerminalId(terminal_id),
        }),
        (arb_uuid(), prop::option::of(any::<u64>()), any::<u32>(),).prop_map(
            |(terminal_id, before, max_bytes)| RequestBody::ReadHistory {
                terminal_id: TerminalId(terminal_id),
                before: before.map(StreamSeq),
                max_bytes,
            }
        ),
        arb_uuid().prop_map(|terminal_id| RequestBody::KillTerminal {
            terminal_id: TerminalId(terminal_id),
        }),
        Just(RequestBody::GetServerConfig),
        arb_server_config().prop_map(|config| RequestBody::SetServerConfig { config }),
        (arb_uuid(), arb_bytes(256)).prop_map(|(terminal_id, bytes)| RequestBody::SendInput {
            terminal_id: TerminalId(terminal_id),
            bytes,
        }),
        (arb_uuid(), any::<u16>(), any::<u16>()).prop_map(|(terminal_id, cols, rows)| {
            RequestBody::ResizeTerminal {
                terminal_id: TerminalId(terminal_id),
                cols,
                rows,
            }
        },),
    ]
}

fn arb_response_body() -> impl Strategy<Value = ResponseBody> {
    prop_oneof![
        prop::collection::vec(arb_terminal_info(), 0..=4)
            .prop_map(|terminals| ResponseBody::ListTerminals { terminals }),
        arb_terminal_info().prop_map(|info| ResponseBody::CreateTerminal { info }),
        (
            any::<u32>(),
            arb_terminal_info(),
            any::<u64>(),
            any::<u64>(),
            arb_bytes(320),
            any::<bool>(),
        )
            .prop_map(
                |(stream_id, info, start_seq, end_seq, render_bytes, has_more_history)| {
                    ResponseBody::OpenTerminal {
                        ack: OpenTerminalAck {
                            stream_id: StreamId(stream_id),
                            info,
                            start_seq: StreamSeq(start_seq),
                            end_seq: StreamSeq(end_seq),
                            render_bytes,
                            has_more_history,
                        },
                    }
                },
            ),
        (
            arb_uuid(),
            any::<u64>(),
            any::<u64>(),
            arb_bytes(320),
            any::<bool>()
        )
            .prop_map(|(terminal_id, start_seq, end_seq, bytes, has_more)| {
                ResponseBody::ReadHistory {
                    page: HistoryPage {
                        terminal_id: TerminalId(terminal_id),
                        start_seq: StreamSeq(start_seq),
                        end_seq: StreamSeq(end_seq),
                        bytes,
                        has_more,
                    },
                }
            },),
        Just(ResponseBody::KillTerminal),
        arb_server_config().prop_map(|config| ResponseBody::GetServerConfig { config }),
        arb_server_config().prop_map(|config| ResponseBody::SetServerConfig { config }),
        Just(ResponseBody::SendInput),
        Just(ResponseBody::ResizeTerminal),
    ]
}

fn arb_protocol_error() -> impl Strategy<Value = ProtocolError> {
    prop_oneof![
        Just(ProtocolError::UnknownTerminal),
        Just(ProtocolError::Unauthorized),
        Just(ProtocolError::BackpressureExceeded),
        Just(ProtocolError::ProtocolMismatch),
        Just(ProtocolError::ResourceExhausted),
        arb_string(32).prop_map(ProtocolError::InvalidParam),
        Just(ProtocolError::ResumeStale),
        Just(ProtocolError::RateLimited),
        arb_string(32).prop_map(ProtocolError::Other),
    ]
}

fn arb_response_error() -> impl Strategy<Value = ResponseError> {
    (arb_protocol_error(), arb_string(64))
        .prop_map(|(code, message)| ResponseError { code, message })
}

fn arb_event_body() -> impl Strategy<Value = EventBody> {
    prop_oneof![
        Just(EventBody::Connected),
        arb_string(64).prop_map(|reason| EventBody::Disconnected { reason }),
        arb_terminal_info().prop_map(|info| EventBody::TerminalCreated { info }),
        (arb_protocol_error(), arb_string(64))
            .prop_map(|(error, message)| EventBody::Error { error, message }),
    ]
}

fn arb_generic_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        (any::<u32>(), arb_request_body()).prop_map(|(id, body)| {
            FrameBody::Request(RequestFrame {
                id: RequestId(id),
                body,
            })
        }),
        prop_oneof![
            (any::<u32>(), arb_response_body()).prop_map(|(id, body)| {
                FrameBody::Response(ResponseFrame {
                    id: RequestId(id),
                    result: Ok(body),
                })
            }),
            (any::<u32>(), arb_response_error()).prop_map(|(id, error)| {
                FrameBody::Response(ResponseFrame {
                    id: RequestId(id),
                    result: Err(error),
                })
            }),
        ],
        (
            any::<u32>(),
            any::<u64>(),
            prop::option::of(any::<u32>()),
            arb_bytes(256),
            any::<bool>(),
        )
            .prop_map(|(stream_id, seq, offset, bytes, last)| {
                FrameBody::StreamData(StreamDataFrame {
                    stream_id: StreamId(stream_id),
                    seq: StreamSeq(seq),
                    offset,
                    bytes,
                    last,
                })
            }),
        arb_event_body().prop_map(|body| FrameBody::Event(EventFrame { body })),
    ]
}

fn arb_hello() -> impl Strategy<Value = Hello> {
    (
        any::<u32>(),
        any::<u32>(),
        prop::collection::vec((arb_uuid(), any::<u64>()), 0..=3),
        any::<u8>(),
    )
        .prop_map(
            |(protocol_min, protocol_max, attachments, resume_flag)| Hello {
                protocol_min,
                protocol_max,
                resume: if resume_flag % 2 == 0 {
                    None
                } else {
                    Some(ResumeToken {
                        session_id: SessionId(Uuid::nil()),
                        attachments: attachments
                            .into_iter()
                            .map(|(terminal, last_seq)| ResumeAttachment {
                                terminal: TerminalId(terminal),
                                last_seq: StreamSeq(last_seq),
                            })
                            .collect(),
                    })
                },
            },
        )
}

fn arb_connection_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        arb_hello().prop_map(FrameBody::Hello),
        arb_hello().prop_map(|hello| FrameBody::HelloOk(HelloOk {
            protocol: hello.protocol_max,
            server_info: ServerInfo {
                server_version: "cli-pocket-daemon 0.1.0".to_string(),
                server_label: None,
            },
            session_id: SessionId(Uuid::nil()),
            resumed: false,
        })),
        any::<u32>().prop_map(|nonce| FrameBody::Ping { nonce }),
        any::<u32>().prop_map(|nonce| FrameBody::Pong { nonce }),
        Just(FrameBody::Bye {
            reason: ByeReason::Normal,
        }),
    ]
}

fn arb_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![arb_generic_frame_body(), arb_connection_frame_body()]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..Default::default() })]

    #[test]
    fn frame_roundtrips_through_postcard(body in arb_frame_body()) {
        let frame = Frame::body(body);
        let bytes = encode_frame(&frame).expect("encode_frame");
        let back = decode_frame(&bytes).expect("decode_frame");
        prop_assert_eq!(frame, back);
    }

    #[test]
    fn relay_ctrl_roundtrips(ctrl in prop_oneof![
        Just(RelayCtrl::ServerRegister {
            server_id: ServerId(Uuid::nil()),
            server_pubkey: ByteBuf::from(vec![0u8; 32]),
            signature: ByteBuf::from(vec![1u8; 64]),
        }),
        Just(RelayCtrl::ServerRegisterOk),
        Just(RelayCtrl::ServerRegisterErr {
            reason: "bad registration".to_string(),
        }),
        Just(RelayCtrl::ServerHeartbeat),
        arb_uuid().prop_map(|server_id| RelayCtrl::ClientConnect {
            server_id: ServerId(server_id),
        }),
        arb_uuid().prop_map(|pair_id| RelayCtrl::PairInbound {
            pair_id: PairId(pair_id),
        }),
        arb_uuid().prop_map(|pair_id| RelayCtrl::PairOpen {
            pair_id: PairId(pair_id),
        }),
        arb_uuid().prop_map(|pair_id| RelayCtrl::PairClose {
            pair_id: PairId(pair_id),
            reason: PairCloseReason::Normal,
        }),
    ]) {
        let bytes = encode_relay_ctrl(&ctrl).expect("encode_relay_ctrl");
        let back = decode_relay(&bytes).expect("decode_relay");
        prop_assert_eq!(back, RelayWire::Ctrl(ctrl));
    }

    #[test]
    fn relay_data_roundtrips(pair_id in arb_uuid(), bytes in arb_bytes(256)) {
        let data = RelayData::Forward {
            pair_id: PairId(pair_id),
            bytes,
        };
        let wire = encode_relay_data(&data).expect("encode_relay_data");
        let back = decode_relay(&wire).expect("decode_relay");
        prop_assert_eq!(back, RelayWire::Data(data));
    }
}

#[test]
fn relay_decode_reports_empty_and_unknown_discriminator() {
    assert!(matches!(decode_relay(&[]), Err(CodecError::Empty)));
    assert!(matches!(
        decode_relay(&[0xff]),
        Err(CodecError::UnknownDiscriminator(0xff))
    ));
}

#[test]
fn empty_stream_chunk_roundtrips() {
    let frame = Frame::body(FrameBody::StreamData(StreamDataFrame {
        stream_id: StreamId(7),
        seq: StreamSeq(55),
        offset: Some(0),
        bytes: ByteBuf::from(Vec::new()),
        last: true,
    }));

    let bytes = encode_frame(&frame).expect("encode_frame");
    let back = decode_frame(&bytes).expect("decode_frame");

    assert_eq!(frame, back);
}

#[test]
fn large_request_offset_and_seq_roundtrip() {
    let frame = Frame::body(FrameBody::StreamData(StreamDataFrame {
        stream_id: StreamId(u32::MAX),
        seq: StreamSeq(u64::MAX),
        offset: Some(u32::MAX),
        bytes: ByteBuf::from(vec![1, 2, 3]),
        last: false,
    }));
    let request = Frame::body(FrameBody::Request(RequestFrame {
        id: RequestId(u32::MAX),
        body: RequestBody::ReadHistory {
            terminal_id: TerminalId(Uuid::nil()),
            before: Some(StreamSeq(u64::MAX)),
            max_bytes: u32::MAX,
        },
    }));

    for frame in [frame, request] {
        let bytes = encode_frame(&frame).expect("encode_frame");
        let back = decode_frame(&bytes).expect("decode_frame");
        assert_eq!(frame, back);
    }
}

#[test]
fn error_response_roundtrips() {
    let frame = Frame::body(FrameBody::Response(ResponseFrame {
        id: RequestId(42),
        result: Err(ResponseError {
            code: ProtocolError::UnknownTerminal,
            message: "terminal not found".to_owned(),
        }),
    }));

    let bytes = encode_frame(&frame).expect("encode_frame");
    let back = decode_frame(&bytes).expect("decode_frame");

    assert_eq!(frame, back);
}
