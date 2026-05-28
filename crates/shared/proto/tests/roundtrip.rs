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
        prop::option::of(0u32..=1_000_000),
    )
        .prop_map(
            |(cols, rows, cwd, cmd, env, scrollback_bytes)| TerminalCreateParams {
                cols,
                rows,
                cwd,
                cmd,
                env,
                scrollback_bytes,
            },
        )
}

fn arb_exit_info() -> impl Strategy<Value = ExitInfo> {
    (
        prop::option::of(any::<i32>()),
        prop::option::of(any::<u32>()),
        any::<u64>(),
    )
        .prop_map(|(code, signal, at_unix_ms)| ExitInfo {
            code,
            signal,
            at_unix_ms,
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

fn arb_snapshot() -> impl Strategy<Value = Snapshot> {
    (
        any::<u16>(),
        any::<u16>(),
        any::<u16>(),
        any::<u16>(),
        any::<u64>(),
        arb_bytes(128),
    )
        .prop_map(
            |(cols, rows, cursor_x, cursor_y, head_seq, bytes)| Snapshot {
                cols,
                rows,
                anchor_state: AnchorState {
                    cursor: (cursor_x, cursor_y),
                    sgr: SgrAttrs::default(),
                    modes: TerminalModes {
                        deccmm_cursor_keys: false,
                        autowrap: true,
                        alt_screen: false,
                        bracketed_paste: false,
                        mouse_reporting: MouseMode::Off,
                        origin_mode: false,
                    },
                    charset: CharsetState::default(),
                    title: None,
                },
                bytes,
                head_seq: StreamSeq(head_seq),
            },
        )
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
                host_label: None,
            },
            session_id: SessionId(Uuid::nil()),
            resumed: false,
        })),
        any::<u32>().prop_map(|nonce| FrameBody::Ping { nonce }),
        any::<u32>().prop_map(|nonce| FrameBody::Pong { nonce }),
        Just(FrameBody::Bye {
            reason: ByeReason::Normal,
        }),
        (any::<u32>(), arb_terminal_create_params())
            .prop_map(|(request_id, params)| FrameBody::TerminalCreate { request_id, params }),
    ]
}

fn arb_terminal_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        (any::<u32>(), arb_uuid(), any::<u32>()).prop_map(|(request_id, terminal, stream)| {
            FrameBody::TerminalCreateOk {
                request_id,
                terminal: TerminalId(terminal),
                stream: StreamId(stream),
            }
        }),
        any::<u32>().prop_map(|request_id| FrameBody::TerminalCreateErr {
            request_id,
            error: ProtocolError::UnknownTerminal,
        }),
        (any::<u32>(), arb_uuid(), prop::option::of(any::<u64>())).prop_map(
            |(request_id, terminal, since)| FrameBody::TerminalAttach {
                request_id,
                terminal: TerminalId(terminal),
                since: since.map(StreamSeq),
            },
        ),
        (
            any::<u32>(),
            arb_snapshot(),
            any::<u64>(),
            any::<u32>(),
            any::<u32>(),
        )
            .prop_map(|(request_id, snapshot, head_seq, stream, initial_window)| {
                FrameBody::TerminalAttachOk {
                    request_id,
                    snapshot,
                    head_seq: StreamSeq(head_seq),
                    stream: StreamId(stream),
                    initial_window,
                }
            }),
        any::<u32>().prop_map(|request_id| FrameBody::TerminalAttachErr {
            request_id,
            error: ProtocolError::Unauthorized,
        }),
        (any::<u32>(), arb_uuid()).prop_map(|(request_id, terminal)| FrameBody::TerminalKill {
            request_id,
            terminal: TerminalId(terminal),
        }),
        any::<u32>().prop_map(|request_id| FrameBody::TerminalKillOk { request_id }),
        any::<u32>().prop_map(|request_id| FrameBody::TerminalKillErr {
            request_id,
            error: ProtocolError::ResourceExhausted,
        }),
        any::<u32>().prop_map(|request_id| FrameBody::TerminalList { request_id }),
        prop::collection::vec(arb_terminal_info(), 0..=4).prop_map(|terminals| {
            FrameBody::TerminalListOk {
                request_id: 1,
                terminals,
            }
        }),
        (arb_uuid(), arb_exit_info()).prop_map(|(terminal, exit)| FrameBody::TerminalExit {
            terminal: TerminalId(terminal),
            exit,
        }),
    ]
}

fn arb_data_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        (any::<u32>(), arb_bytes(256), any::<u64>()).prop_map(|(stream, bytes, seq)| {
            FrameBody::Output {
                stream: StreamId(stream),
                seq: StreamSeq(seq),
                bytes,
            }
        }),
        (any::<u32>(), arb_bytes(256)).prop_map(|(stream, bytes)| FrameBody::Input {
            stream: StreamId(stream),
            bytes,
        }),
    ]
}

fn arb_flow_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        (any::<u32>(), any::<u16>(), any::<u16>()).prop_map(|(stream, cols, rows)| {
            FrameBody::Resize {
                stream: StreamId(stream),
                cols,
                rows,
            }
        }),
        (any::<u32>(), any::<u32>()).prop_map(|(stream, credit)| FrameBody::Window {
            stream: StreamId(stream),
            credit,
        }),
    ]
}

fn arb_frame_body() -> impl Strategy<Value = FrameBody> {
    prop_oneof![
        arb_connection_frame_body(),
        arb_terminal_frame_body(),
        arb_data_frame_body(),
        arb_flow_frame_body(),
    ]
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
        Just(RelayCtrl::HostRegister {
            host_id: HostId(Uuid::nil()),
            host_pubkey: ByteBuf::from(vec![0u8; 32]),
            signature: ByteBuf::from(vec![1u8; 64]),
        }),
        Just(RelayCtrl::HostRegisterOk),
        Just(RelayCtrl::HostRegisterErr {
            reason: "bad registration".to_string(),
        }),
        Just(RelayCtrl::HostHeartbeat),
        arb_uuid().prop_map(|host_id| RelayCtrl::ClientConnect {
            host_id: HostId(host_id),
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
