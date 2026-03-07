pub mod agent;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Session {
    pub id: String,
    pub model_id: String,
    pub abort: tokio::sync::watch::Sender<bool>,
}

pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a new session and return a watch receiver for abort signaling.
    /// If a session with the same ID already exists, it is aborted first.
    pub async fn start_session(
        &self,
        id: String,
        model_id: String,
    ) -> tokio::sync::watch::Receiver<bool> {
        let mut sessions = self.sessions.lock().await;

        // Abort existing session with same ID if present
        if let Some(existing) = sessions.remove(&id) {
            let _ = existing.abort.send(true);
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        sessions.insert(
            id.clone(),
            Session {
                id,
                model_id,
                abort: tx,
            },
        );
        rx
    }

    /// Signal abort to a running session.
    pub async fn stop_session(&self, id: &str) {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(id) {
            let _ = session.abort.send(true);
        }
    }

    /// Remove a session from the manager (called after stream completes).
    pub async fn remove_session(&self, id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(id);
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
