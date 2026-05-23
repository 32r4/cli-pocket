//! A thin facade over [`ClientSession`] that owns the session + event-receiver
//! pair and provides `Clone`-able access suitable for Tauri command handlers.
//!
//! `ClientSession` uses `Rc<RefCell<…>>` internally and is therefore `!Send`.
//! `SessionHandle` mirrors that approach: it wraps `Rc<RefCell<State>>` so it
//! too is `!Send`, which is fine for Tauri's `current_thread` Tokio runtime.
//! Use `SessionHandle::new_disconnected()` on startup; call `connect(builder)`
//! once you have a fully-built `SessionBuilder`.

use std::cell::RefCell;
use std::rc::Rc;

use cli_pocket_client_core::session::SessionBuilder;
use cli_pocket_client_core::session::SessionSpawner;
use cli_pocket_client_core::{ClientEvent, ClientSession, Clock, KeyValueStore, Rng, TerminalHandle, Transport};
use cli_pocket_proto::TerminalCreateParams;
use futures_channel::mpsc;

// ── internal state ────────────────────────────────────────────────────────────

struct State {
    session: Option<ClientSession>,
    events: Option<mpsc::Receiver<ClientEvent>>,
}

// ── public facade ─────────────────────────────────────────────────────────────

/// Clonable, `!Send` facade over a [`ClientSession`].
///
/// All clones share the same internal state cell — they are lightweight
/// reference-counted handles into a single `RefCell<State>`.
#[derive(Clone)]
pub struct SessionHandle {
    inner: Rc<RefCell<State>>,
}

impl SessionHandle {
    /// Create a handle that is not yet connected to any server.
    pub fn new_disconnected() -> Self {
        Self {
            inner: Rc::new(RefCell::new(State {
                session: None,
                events: None,
            })),
        }
    }

    /// Returns `true` if a live `ClientSession` is stored.
    pub fn is_connected(&self) -> bool {
        self.inner.borrow().session.is_some()
    }

    /// Take the event receiver out of the handle.
    ///
    /// Returns `None` if either no session has been started yet, or the
    /// receiver was already taken (e.g. by the event-pump task).
    pub fn take_event_rx(&self) -> Option<mpsc::Receiver<ClientEvent>> {
        self.inner.borrow_mut().events.take()
    }

    /// Start a session from a fully-configured [`SessionBuilder`] and store
    /// both the [`ClientSession`] and the event receiver.
    ///
    /// Any existing session is dropped first (which closes the command
    /// channels and causes `run_session_loop` to exit on the next iteration).
    pub fn connect<T, C, R, K, S>(&self, builder: SessionBuilder<T, C, R, K, S>)
    where
        T: Transport + 'static,
        C: Clock + 'static,
        R: Rng + 'static,
        K: KeyValueStore + 'static,
        S: SessionSpawner + 'static,
    {
        let (session, events_rx) = builder.start();
        let mut state = self.inner.borrow_mut();
        state.session = Some(session);
        state.events = Some(events_rx);
    }

    /// Forward a `create_terminal` call to the underlying session.
    ///
    /// Holds the `RefCell` borrow across the `await` point, which is safe
    /// because `ClientSession::create_terminal` only clones an internal
    /// `mpsc::Sender` and does not re-enter this `RefCell`.
    pub async fn create_terminal(&self, params: TerminalCreateParams) -> Result<(), String> {
        // We hold the borrow across the await.  This is safe: the future
        // produced by `ClientSession::create_terminal` sends into an mpsc
        // channel — it never re-borrows `self.inner`.
        let result = {
            let state = self.inner.borrow();
            match &state.session {
                None => return Err("not connected".to_owned()),
                Some(session) => session.create_terminal(params).await,
            }
        };
        result.map_err(|e| e.to_string())
    }

    /// Return a clone of the `TerminalHandle` if the session has one attached.
    pub async fn terminal(&self) -> Option<TerminalHandle> {
        let state = self.inner.borrow();
        match &state.session {
            None => None,
            Some(session) => session.terminal().await,
        }
    }

    /// Drop the underlying session, which closes the command channels and
    /// causes `run_session_loop` to exit on its next iteration.
    pub fn shutdown(&self) {
        let mut state = self.inner.borrow_mut();
        state.session = None;
        state.events = None;
    }
}
