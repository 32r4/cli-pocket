use cli_pocket_proto::{Capabilities, HostId, ResumeToken};
use futures_channel::mpsc;
use futures_util::lock::Mutex;
use std::rc::Rc;

use crate::terminal::{TerminalCmd, TerminalHandle};
use crate::{ClientEvent, ClientIdentity};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub endpoint: SessionEndpoint,
    pub server_public: [u8; 32],
    pub resume_token: Option<ResumeToken>,
    pub capabilities: Capabilities,
    pub backoff: (u64, u64, u32),
}

#[derive(Debug, Clone)]
pub enum SessionEndpoint {
    Direct(String),
    Relay {
        url: String,
        host_id: HostId,
        psk_hex: String,
    },
}

#[allow(dead_code)]
pub struct ClientSession {
    pub(crate) identity: ClientIdentity,
    pub(crate) config: SessionConfig,
    pub(crate) events_tx: mpsc::Sender<ClientEvent>,
    pub(crate) cmd_tx: mpsc::Sender<TerminalCmd>,
    pub(crate) terminal: Rc<Mutex<Option<TerminalHandle>>>,
}
