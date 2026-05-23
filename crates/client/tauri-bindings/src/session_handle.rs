//! A thin facade over [`ClientSession`] that owns the session + event-receiver
//! pair and provides `Clone + Send + Sync` access suitable for Tauri command handlers.
//!
//! `ClientSession` uses `Rc<RefCell<…>>` internally and is therefore `!Send`.
//! `SessionHandle` wraps a `tokio::sync::mpsc::Sender<SessionCommand>` which IS
//! `Send + Sync`, satisfying Tauri's `State<T>` requirements. A background task
//! (spawned on a dedicated thread with a `LocalSet`) owns the actual `ClientSession`
//! and processes commands via message passing.

use cli_pocket_client_core::session::SessionBuilder;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{ClientEvent, ClientSession, Clock, KeyValueStore, Rng, Transport};
use cli_pocket_proto::TerminalCreateParams;
use futures_channel::mpsc as futures_mpsc;
use std::thread;
use tokio::sync::{mpsc, oneshot};

type SessionStart = (ClientSession, futures_mpsc::Receiver<ClientEvent>);
type SessionFactory = Box<dyn FnOnce(&LocalSpawner) -> SessionStart + Send>;

// ── command enum ──────────────────────────────────────────────────────────────

/// Commands sent from `SessionHandle` methods to the background actor task.
///
/// Note: We can't send the `SessionBuilder` directly because it contains `!Send`
/// types. Instead, we send a `ConnectRequest` that contains all the `Send` parts,
/// and the actor constructs the builder on its own thread.
enum SessionCommand {
    /// Check if a session is currently connected.
    IsConnected { reply: oneshot::Sender<bool> },

    /// Connect using a pre-built SessionBuilder.
    /// The builder is constructed on the actor thread from the provided factory.
    Connect {
        factory: SessionFactory,
        reply: oneshot::Sender<Result<(), String>>,
    },

    /// Create a new terminal with the given parameters.
    CreateTerminal {
        params: TerminalCreateParams,
        reply: oneshot::Sender<Result<(), String>>,
    },

    /// Shutdown the session (drop it).
    Shutdown { reply: oneshot::Sender<()> },
}

/// A spawner that can be used from the actor thread to spawn local futures.
#[derive(Clone)]
pub struct LocalSpawner {
    // Marker to ensure this is only used on the actor thread
    _private: (),
}

impl SessionSpawner for LocalSpawner {
    fn spawn(&self, fut: futures_util::future::LocalBoxFuture<'static, ()>) {
        // We're already on the LocalSet thread, so we can spawn_local
        tokio::task::spawn_local(fut);
    }
}

// ── public facade ─────────────────────────────────────────────────────────────

/// Clonable, `Send + Sync` facade over a [`ClientSession`].
///
/// All clones share the same underlying actor task via an `mpsc::Sender`.
/// The actual `ClientSession` (which is `!Send`) lives inside the background
/// task and never crosses thread boundaries.
#[derive(Clone)]
pub struct SessionHandle {
    cmd_tx: mpsc::Sender<SessionCommand>,
}

impl SessionHandle {
    /// Spawn a new session actor on a dedicated thread and return a handle to it.
    ///
    /// The `event_tx` channel is used to forward `ClientEvent`s from the
    /// underlying `ClientSession` to the caller (e.g., for Tauri event emission).
    ///
    /// Returns the `SessionHandle` which can be cloned and shared across threads.
    pub fn spawn(event_tx: mpsc::Sender<ClientEvent>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCommand>(32);

        // Spawn a dedicated thread with its own LocalSet for the !Send ClientSession
        thread::Builder::new()
            .name("session-actor".to_owned())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for session actor");

                let local = tokio::task::LocalSet::new();
                local.block_on(&rt, actor_loop(cmd_rx, event_tx));
            })
            .expect("failed to spawn session actor thread");

        Self { cmd_tx }
    }

    /// Returns `true` if a live `ClientSession` is stored.
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

    /// Start a session from a fully-configured [`SessionBuilder`] and store
    /// both the [`ClientSession`] and the event receiver.
    ///
    /// Any existing session is dropped first (which closes the command
    /// channels and causes `run_session_loop` to exit on the next iteration).
    pub async fn connect<T, C, R, K, F>(&self, builder_factory: F) -> Result<(), String>
    where
        T: Transport + 'static,
        C: Clock + 'static,
        R: Rng + 'static,
        K: KeyValueStore + 'static,
        F: FnOnce(&LocalSpawner) -> SessionBuilder<T, C, R, K, LocalSpawner> + Send + 'static,
    {
        // Type-erase the builder factory
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

        reply_rx.await.map_err(|_| "actor dropped reply")?
    }

    /// Forward a `create_terminal` call to the underlying session.
    pub async fn create_terminal(&self, params: TerminalCreateParams) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CreateTerminal {
                params,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "actor closed".to_owned())?;

        reply_rx.await.map_err(|_| "actor dropped reply")?
    }

    /// Drop the underlying session, which closes the command channels and
    /// causes `run_session_loop` to exit on its next iteration.
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

// ── actor loop ────────────────────────────────────────────────────────────────

/// Internal state for the actor task.
struct ActorState {
    session: Option<ClientSession>,
    /// The event receiver from the ClientSession. We forward events to the
    /// external event_tx channel.
    events_rx: Option<futures_mpsc::Receiver<ClientEvent>>,
}

/// The background actor loop that owns the `!Send` `ClientSession`.
///
/// This task runs on a dedicated thread with a `LocalSet` and processes
/// commands from the `SessionHandle` via message passing.
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
        // If we have an event receiver, select between commands and events
        if let Some(ref mut events_rx) = state.events_rx {
            tokio::select! {
                biased;

                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else {
                        // All SessionHandle clones dropped, exit
                        break;
                    };
                    if !handle_command(cmd, &mut state, &spawner).await {
                        break;
                    }
                }

                event = events_rx.next() => {
                    if let Some(event) = event {
                        // Forward event to the external channel
                        let _ = event_tx.send(event).await;
                    } else {
                        // Event stream closed, session ended
                        state.session = None;
                        state.events_rx = None;
                    }
                }
            }
        } else {
            // No event receiver, just wait for commands
            let Some(cmd) = cmd_rx.recv().await else {
                // All SessionHandle clones dropped, exit
                break;
            };
            if !handle_command(cmd, &mut state, &spawner).await {
                break;
            }
        }
    }
}

/// Handle a single command. Returns `false` if the actor should exit.
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
            // Drop any existing session first
            state.session = None;
            state.events_rx = None;

            // Start the new session using the factory
            let (session, events_rx) = factory(spawner);
            state.session = Some(session);
            state.events_rx = Some(events_rx);

            let _ = reply.send(Ok(()));
        }

        SessionCommand::CreateTerminal { params, reply } => {
            let result = match &state.session {
                None => Err("not connected".to_owned()),
                Some(session) => session
                    .create_terminal(params)
                    .await
                    .map_err(|e| e.to_string()),
            };
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
