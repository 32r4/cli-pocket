//! A thin facade over [`ClientSession`] that owns the session + event-receiver
//! pair and provides `Clone + Send + Sync` access suitable for Tauri command handlers.

use bytes::Bytes;
use cli_pocket_client_core::session::SessionBuilder;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{ClientEvent, ClientSession, Clock, KeyValueStore, Rng, Transport};
use cli_pocket_proto::{TerminalCreateParams, TerminalId};
use futures_channel::mpsc as futures_mpsc;
use std::thread;
use tokio::sync::{mpsc, oneshot};

type SessionStart = (ClientSession, futures_mpsc::Receiver<ClientEvent>);
type SessionFactory = Box<dyn FnOnce(&LocalSpawner) -> SessionStart + Send>;

enum SessionCommand {
    IsConnected {
        reply: oneshot::Sender<bool>,
    },
    Connect {
        factory: SessionFactory,
        reply: oneshot::Sender<Result<(), String>>,
    },
    CreateTerminal {
        params: TerminalCreateParams,
        reply: oneshot::Sender<Result<(), String>>,
    },
    SendInput {
        terminal_id: TerminalId,
        bytes: Bytes,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Resize {
        terminal_id: TerminalId,
        cols: u16,
        rows: u16,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Kill {
        terminal_id: TerminalId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub struct LocalSpawner {
    _private: (),
}

impl SessionSpawner for LocalSpawner {
    fn spawn(&self, fut: futures_util::future::LocalBoxFuture<'static, ()>) {
        tokio::task::spawn_local(fut);
    }
}

#[derive(Clone)]
pub struct SessionHandle {
    cmd_tx: mpsc::Sender<SessionCommand>,
}

impl SessionHandle {
    pub fn spawn(event_tx: mpsc::Sender<ClientEvent>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCommand>(32);

        thread::Builder::new()
            .name("session-actor".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for session actor");

                let local = tokio::task::LocalSet::new();
                local.block_on(&runtime, actor_loop(cmd_rx, event_tx));
            })
            .expect("failed to spawn session actor thread");

        Self { cmd_tx }
    }

    pub async fn is_connected(&self) -> bool {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::IsConnected { reply: reply_tx })
            .await
            .is_err()
        {
            return false;
        }
        reply_rx.await.unwrap_or(false)
    }

    pub async fn connect<T, C, R, K, F>(&self, builder_factory: F) -> Result<(), String>
    where
        T: Transport + 'static,
        C: Clock + 'static,
        R: Rng + 'static,
        K: KeyValueStore + 'static,
        F: FnOnce(&LocalSpawner) -> SessionBuilder<T, C, R, K, LocalSpawner> + Send + 'static,
    {
        let factory: SessionFactory = Box::new(move |spawner| {
            let builder = builder_factory(spawner);
            builder.start()
        });

        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Connect {
                factory,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx
            .await
            .map_err(|_| "actor dropped reply".to_owned())?
    }

    pub async fn create_terminal(&self, params: TerminalCreateParams) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CreateTerminal {
                params,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx
            .await
            .map_err(|_| "actor dropped reply".to_owned())?
    }

    pub async fn send_input(&self, terminal_id: TerminalId, bytes: Bytes) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SendInput {
                terminal_id,
                bytes,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx
            .await
            .map_err(|_| "actor dropped reply".to_owned())?
    }

    pub async fn resize(
        &self,
        terminal_id: TerminalId,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Resize {
                terminal_id,
                cols,
                rows,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx
            .await
            .map_err(|_| "actor dropped reply".to_owned())?
    }

    pub async fn kill(&self, terminal_id: TerminalId) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Kill {
                terminal_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx
            .await
            .map_err(|_| "actor dropped reply".to_owned())?
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::Shutdown { reply: reply_tx })
            .await
            .is_ok()
        {
            let _ = reply_rx.await;
        }
    }
}

struct ActorState {
    session: Option<ClientSession>,
    events_rx: Option<futures_mpsc::Receiver<ClientEvent>>,
}

async fn actor_loop(
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    event_tx: mpsc::Sender<ClientEvent>,
) {
    use futures_util::StreamExt;

    let mut state = ActorState {
        session: None,
        events_rx: None,
    };
    let spawner = LocalSpawner { _private: () };

    loop {
        if let Some(ref mut events_rx) = state.events_rx {
            tokio::select! {
                biased;

                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else {
                        break;
                    };
                    if !handle_command(cmd, &mut state, &spawner).await {
                        break;
                    }
                }

                event = events_rx.next() => {
                    if let Some(event) = event {
                        let _ = event_tx.send(event).await;
                    } else {
                        state.session = None;
                        state.events_rx = None;
                    }
                }
            }
        } else {
            let Some(cmd) = cmd_rx.recv().await else {
                break;
            };
            if !handle_command(cmd, &mut state, &spawner).await {
                break;
            }
        }
    }
}

async fn handle_command(
    cmd: SessionCommand,
    state: &mut ActorState,
    spawner: &LocalSpawner,
) -> bool {
    match cmd {
        SessionCommand::IsConnected { reply } => {
            let _ = reply.send(state.session.is_some());
        }
        SessionCommand::Connect { factory, reply } => {
            state.session = None;
            state.events_rx = None;

            let (session, events_rx) = factory(spawner);
            state.session = Some(session);
            state.events_rx = Some(events_rx);

            let _ = reply.send(Ok(()));
        }
        SessionCommand::CreateTerminal { params, reply } => {
            let result = match &state.session {
                Some(session) => session
                    .create_terminal(params)
                    .await
                    .map_err(|error| error.to_string()),
                None => Err("not connected".to_owned()),
            };
            let _ = reply.send(result);
        }
        SessionCommand::SendInput {
            terminal_id,
            bytes,
            reply,
        } => {
            let result =
                with_active_terminal(state.session.as_ref(), terminal_id, |handle| async move {
                    handle
                        .write_input(bytes)
                        .await
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = reply.send(result);
        }
        SessionCommand::Resize {
            terminal_id,
            cols,
            rows,
            reply,
        } => {
            let result =
                with_active_terminal(state.session.as_ref(), terminal_id, |handle| async move {
                    handle
                        .resize(cols, rows)
                        .await
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = reply.send(result);
        }
        SessionCommand::Kill { terminal_id, reply } => {
            let result =
                with_active_terminal(state.session.as_ref(), terminal_id, |handle| async move {
                    handle.kill().await.map_err(|error| error.to_string())
                })
                .await;
            let _ = reply.send(result);
        }
        SessionCommand::Shutdown { reply } => {
            state.session = None;
            state.events_rx = None;
            let _ = reply.send(());
        }
    }

    true
}

async fn with_active_terminal<F, Fut>(
    session: Option<&ClientSession>,
    requested_terminal_id: TerminalId,
    action: F,
) -> Result<(), String>
where
    F: FnOnce(cli_pocket_client_core::TerminalHandle) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let session = session.ok_or_else(|| "not connected".to_owned())?;
    let handle = session
        .terminal()
        .await
        .ok_or_else(|| "no active terminal".to_owned())?;

    if handle.terminal_id() != requested_terminal_id {
        return Err(format!(
            "terminal_id does not match active terminal {}",
            handle.terminal_id().0
        ));
    }

    action(handle).await
}
