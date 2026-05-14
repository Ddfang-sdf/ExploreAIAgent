use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::common::config::AppConfig;
use crate::orchestrator::orchestrator::Orchestrator;
use crate::conversation::manager::ConversationManager;

// ============================================================================
// Data structures (design doc section 3.5)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    pub question: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub code: i32,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ChatResponse {
    pub fn error(code: i32, msg: &str) -> Self {
        ChatResponse {
            code,
            session_id: String::new(),
            answer: None,
            error: Some(msg.to_string()),
        }
    }
}

// ============================================================================
// AppState (design doc section 3.3.1)
// ============================================================================

pub struct AppState {
    pub orchestrator: Orchestrator,
    pub conversation_manager: ConversationManager,
    pub config: AppConfig,
}

impl AppState {
    pub fn new(
        orchestrator: Orchestrator,
        conversation_manager: ConversationManager,
        config: AppConfig,
    ) -> Self {
        AppState { orchestrator, conversation_manager, config }
    }
}

// ============================================================================
// Public API (design doc section 3.3.2)
// ============================================================================

pub async fn handle_chat_request(
    body: ChatRequest,
    state: &AppState,
) -> ChatResponse {
    // Step 1: validate question
    if body.question.trim().is_empty() {
        return ChatResponse::error(2, "question is required");
    }

    // Step 2: get or generate session_id
    let session_id = body
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

    // call Orchestrator
    match state.orchestrator.run(&body.question, &[], Arc::new(AtomicBool::new(false))).await {
        Ok((answer, _)) => ChatResponse {
            code: 0,
            session_id,
            answer: Some(answer),
            error: None,
        },
        Err(e) => {
            let code = if e.contains("LLM") || e.contains("retry") {
                3
            } else if e.contains("context") || e.contains("ECT") {
                4
            } else {
                5
            };
            ChatResponse {
                code,
                session_id,
                answer: None,
                error: Some(e),
            }
        }
    }
}

pub fn health_response() -> serde_json::Value {
    serde_json::json!({"status": "ok"})
}
