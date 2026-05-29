use crate::terminal::{ClientId, SessionId, StreamSeq, TerminalId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub resume: Option<ResumeToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub protocol: u32,
    pub server_info: ServerInfo,
    pub session_id: SessionId,
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub server_version: String,
    pub server_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeToken {
    pub session_id: SessionId,
    pub attachments: Vec<ResumeAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeAttachment {
    pub terminal: TerminalId,
    pub last_seq: StreamSeq,
}

/// `HelloOk` carries the server-assigned `SessionId`. On reconnect, the client
/// presents a fresh `ResumeToken` built from per-terminal `last_seq` values.
/// `ClientId` is conveyed out of band via Noise static-key authentication.
#[allow(dead_code)]
fn _client_id_only_via_static_key(_: ClientId) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{SessionId, StreamSeq, TerminalId};

    #[test]
    fn resume_token_roundtrips_through_postcard() {
        let token = ResumeToken {
            session_id: SessionId::new(),
            attachments: vec![ResumeAttachment {
                terminal: TerminalId::new(),
                last_seq: StreamSeq(42),
            }],
        };

        let bytes = postcard::to_allocvec(&token).unwrap();
        let back: ResumeToken = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(token, back);
    }
}
