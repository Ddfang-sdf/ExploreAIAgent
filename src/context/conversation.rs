use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::common::config::ConversationConfig;

pub const CONVERSATION_ROUND_THRESHOLD: usize = 10;
pub const CONVERSATION_TOKEN_THRESHOLD: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub round: u32,
    pub user_question: String,
    pub answer_summary: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    pub total_rounds: u32,
    pub summarized_rounds: u32,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    pub session_id: String,
    pub conversation_history: Vec<ConversationRecord>,
    pub conversation_summary: String,
    pub metadata: ConversationMetadata,
}

#[derive(Clone)]
pub struct ConversationContextTool {
    context: ConversationContext,
    round_threshold: usize,
    token_threshold: usize,
}

impl ConversationContextTool {
    pub fn new(session_id: String) -> Self {
        ConversationContextTool {
            context: ConversationContext {
                session_id,
                conversation_history: Vec::new(),
                conversation_summary: String::new(),
                metadata: ConversationMetadata {
                    total_rounds: 0,
                    summarized_rounds: 0,
                    last_updated: Utc::now(),
                },
            },
            round_threshold: CONVERSATION_ROUND_THRESHOLD,
            token_threshold: CONVERSATION_TOKEN_THRESHOLD,
        }
    }

    pub fn configure(&mut self, config: &ConversationConfig) {
        self.round_threshold = config.round_threshold;
        self.token_threshold = config.token_threshold;
    }

    pub fn add_record(&mut self, question: String, answer_summary: String) {
        let round = self.context.metadata.total_rounds + 1;
        let record = ConversationRecord {
            round,
            user_question: question,
            answer_summary,
            timestamp: Utc::now(),
        };
        self.context.conversation_history.push(record);
        self.context.metadata.total_rounds = round;
        self.context.metadata.last_updated = Utc::now();
    }

    pub fn get_history(&self) -> &[ConversationRecord] {
        &self.context.conversation_history
    }

    pub fn get_recent_history(&self, n: usize) -> &[ConversationRecord] {
        let history = &self.context.conversation_history;
        if history.len() <= n {
            history
        } else {
            &history[history.len() - n..]
        }
    }

    pub fn get_summary(&self) -> &str {
        &self.context.conversation_summary
    }

    pub fn update_summary(&mut self, summary: String) {
        self.context.conversation_summary = summary;
        self.context.metadata.summarized_rounds = self.context.metadata.total_rounds;
        self.context.metadata.last_updated = Utc::now();
    }

    pub fn total_rounds(&self) -> u32 {
        self.context.metadata.total_rounds
    }

    pub fn summarized_rounds(&self) -> u32 {
        self.context.metadata.summarized_rounds
    }

    pub fn needs_refinement(&self) -> bool {
        if self.context.metadata.total_rounds >= self.round_threshold as u32 {
            return true;
        }
        let serialized = serde_json::to_string(&self.context).unwrap_or_default();
        let estimated_tokens = serialized.len() / 4;
        estimated_tokens > self.token_threshold
    }

    pub fn get_context(&self) -> &ConversationContext {
        &self.context
    }

    pub fn session_id(&self) -> &str {
        &self.context.session_id
    }
}
