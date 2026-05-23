use cli_pocket_tauri_bindings::{ClientEvent, SessionHandle};
use tokio::sync::mpsc;

pub struct AppState {
    pub session: SessionHandle,
    pub event_rx: Option<mpsc::Receiver<ClientEvent>>,
}

impl AppState {
    pub fn new() -> Self {
        // Create a channel for events from the session actor
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(64);

        // Spawn the session actor with the event sender
        let session = SessionHandle::spawn(event_tx);

        Self {
            session,
            event_rx: Some(event_rx),
        }
    }

    /// Take the event receiver out of the state.
    ///
    /// Returns `None` if the receiver was already taken (e.g., by the event-pump task).
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<ClientEvent>> {
        self.event_rx.take()
    }
}
