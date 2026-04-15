use anyhow::Result;

use crate::config::{session_path, write_json_secure};
use crate::models::SessionState;

pub fn load_session() -> Result<SessionState> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(SessionState::default());
    }

    let raw = std::fs::read_to_string(path)?;
    match serde_json::from_str::<SessionState>(&raw) {
        Ok(state) => Ok(state),
        Err(_) => Ok(SessionState::default()),
    }
}

pub fn save_session(state: &SessionState) -> Result<()> {
    let path = session_path()?;
    write_json_secure(&path, state)
}
