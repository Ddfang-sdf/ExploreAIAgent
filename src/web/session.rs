use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::context::exploration::ExplorationContextTool;

pub struct SessionManager {
    _sessions: Mutex<HashMap<String, SessionEntry>>,
    _ttl_secs: u64,
}

struct SessionEntry {
    _ect: ExplorationContextTool,
    _last_access: Instant,
}

impl SessionManager {
    pub fn new(ttl_secs: u64) -> Self {
        SessionManager {
            _sessions: Mutex::new(HashMap::new()),
            _ttl_secs: ttl_secs,
        }
    }

    pub fn get_or_create(&self, _session_id: &str) -> Result<(), String> {
        Err("SessionManager::get_or_create not yet implemented".to_string())
    }

    pub fn cleanup_expired(&self) -> usize {
        0
    }
}
