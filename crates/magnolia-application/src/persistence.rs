use magnolia_domain::WorkspaceDocument;
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub trait PersistencePort: Send + 'static {
    fn load(&mut self) -> Result<Option<WorkspaceDocument>, PersistenceError>;
    fn save(&mut self, document: &WorkspaceDocument) -> Result<(), PersistenceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct PersistenceError {
    pub message: String,
}

impl PersistenceError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Default)]
struct MemoryState {
    document: Option<WorkspaceDocument>,
    saves: Vec<WorkspaceDocument>,
    fail_next_save: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryPersistence {
    state: Arc<Mutex<MemoryState>>,
}

impl InMemoryPersistence {
    #[must_use]
    pub fn with_document(document: WorkspaceDocument) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryState {
                document: Some(document),
                ..MemoryState::default()
            })),
        }
    }

    pub fn latest(&self) -> Result<Option<WorkspaceDocument>, PersistenceError> {
        self.state
            .lock()
            .map(|state| state.document.clone())
            .map_err(|_| PersistenceError::new("in-memory persistence lock poisoned"))
    }

    pub fn save_count(&self) -> Result<usize, PersistenceError> {
        self.state
            .lock()
            .map(|state| state.saves.len())
            .map_err(|_| PersistenceError::new("in-memory persistence lock poisoned"))
    }

    pub fn fail_next_save(&self, message: impl Into<String>) -> Result<(), PersistenceError> {
        self.state
            .lock()
            .map_err(|_| PersistenceError::new("in-memory persistence lock poisoned"))?
            .fail_next_save = Some(message.into());
        Ok(())
    }
}

impl PersistencePort for InMemoryPersistence {
    fn load(&mut self) -> Result<Option<WorkspaceDocument>, PersistenceError> {
        self.latest()
    }

    fn save(&mut self, document: &WorkspaceDocument) -> Result<(), PersistenceError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PersistenceError::new("in-memory persistence lock poisoned"))?;
        if let Some(message) = state.fail_next_save.take() {
            return Err(PersistenceError::new(message));
        }
        state.document = Some(document.clone());
        state.saves.push(document.clone());
        Ok(())
    }
}
