use serde::{Deserialize, Serialize};
use crate::adapter::api_adapter::ApiAdapter;
use crate::context::conversation::ConversationContextTool;

pub const RECENT_HISTORY_LIMIT: usize = 5;
pub const REFINEMENT_HISTORY_LIMIT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationOutput {
    pub conversation_summary: String,
    pub active_topic: String,
    pub recent_history: Vec<crate::context::conversation::ConversationRecord>,
    pub previous_messages: Vec<serde_json::Value>,
}

#[derive(Clone)]
pub struct ConversationManager {
    sessions: std::collections::HashMap<String, ConversationContextTool>,
    previous_messages: std::collections::HashMap<String, Vec<serde_json::Value>>,
    _adapter: ApiAdapter,
}

impl ConversationManager {
    pub fn new(adapter: ApiAdapter) -> Self {
        ConversationManager {
            sessions: std::collections::HashMap::new(),
            previous_messages: std::collections::HashMap::new(),
            _adapter: adapter,
        }
    }

    pub fn init_session(&mut self, session_id: &str) -> &ConversationContextTool {
        if !self.sessions.contains_key(session_id) {
            self.sessions.insert(
                session_id.to_string(),
                ConversationContextTool::new(session_id.to_string()),
            );
        }
        self.sessions.get(session_id).unwrap()
    }

    pub fn get_context(&self, session_id: &str) -> Result<ConversationOutput, String> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        let conversation_summary = session.get_summary().to_string();
        let recent_history: Vec<crate::context::conversation::ConversationRecord> = session
            .get_recent_history(RECENT_HISTORY_LIMIT)
            .to_vec();

        let active_topic = if let Some(last) = recent_history.last() {
            let answer = &last.answer_summary;
            let char_count = std::cmp::min(50, answer.chars().count());
            answer.chars().take(char_count).collect::<String>()
        } else {
            String::new()
        };

        let previous_messages = self.previous_messages.get(session_id).cloned().unwrap_or_default();
        Ok(ConversationOutput {
            conversation_summary,
            active_topic,
            recent_history,
            previous_messages,
        })
    }

    pub fn save_conversation(
        &mut self,
        session_id: &str,
        question: &str,
        answer_summary: &str,
        messages: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        session.add_record(question.to_string(), answer_summary.to_string());
        if !messages.is_empty() {
            self.previous_messages.insert(session_id.to_string(), messages);
        }
        Ok(())
    }

    pub async fn check_and_refine(
        &mut self,
        session_id: &str,
        current_question: &str,
    ) -> Result<(), String> {
        let needs_refinement = {
            let session = self
                .sessions
                .get(session_id)
                .ok_or_else(|| format!("Session not found: {}", session_id))?;
            session.needs_refinement()
        };

        if !needs_refinement {
            return Ok(());
        }

        let (recent_history, existing_summary) = {
            let session = self
                .sessions
                .get(session_id)
                .ok_or_else(|| format!("Session not found: {}", session_id))?;

            let recent: Vec<crate::agents::conversation_refiner::ConversationRoundRecord> = session
                .get_recent_history(REFINEMENT_HISTORY_LIMIT)
                .iter()
                .map(|r| crate::agents::conversation_refiner::ConversationRoundRecord {
                    round: r.round,
                    user_question: r.user_question.clone(),
                    answer_summary: r.answer_summary.clone(),
                    topic: String::new(),
                })
                .collect();

            let summary = session.get_summary().to_string();
            (recent, summary)
        };

        let refiner = crate::agents::conversation_refiner::ConversationRefinerAgent::new();
        let refined = refiner
            .refine(current_question, &recent_history, &existing_summary, &self._adapter)
            .await?;

        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {}", session_id))?;
        session.update_summary(refined.summary);

        Ok(())
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }

    pub fn destroy_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }
}
