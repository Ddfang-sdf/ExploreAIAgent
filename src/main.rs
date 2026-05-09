use std::io;
use std::sync::Arc;

use axum::{Router, routing::{get, post}, extract::State, Json};
use explore_ai_agent::cli;
use explore_ai_agent::common::config::AppConfig;
use explore_ai_agent::context::exploration::ExplorationContextTool;
use explore_ai_agent::web::{ChatRequest, ChatResponse};
use std::collections::HashMap;
use std::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), String> {
    let config = AppConfig::load(None)?;
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "--web" {
        run_web(config).await
    } else {
        cli::run_cli_with_io(&config, io::stdin().lock(), io::stdout()).await
    }
}

struct WebState {
    core: cli::CoreModules,
    sessions: Mutex<HashMap<String, Arc<ExplorationContextTool>>>,
    config: AppConfig,
}

async fn run_web(config: AppConfig) -> Result<(), String> {
    let core = cli::assemble_core(&config)?;
    let state = Arc::new(WebState {
        core,
        sessions: Mutex::new(HashMap::new()),
        config,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/chat", post(chat))
        .with_state(state);

    let addr = "0.0.0.0:3000";
    println!("Explore AI Agent web mode: http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| e.to_string())?;
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn chat(
    State(state): State<Arc<WebState>>,
    Json(body): Json<ChatRequest>,
) -> Json<ChatResponse> {
    if body.question.trim().is_empty() {
        return Json(ChatResponse::error(2, "question is required"));
    }

    let session_id = body.session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

    let ect = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions.entry(session_id.clone()).or_insert_with(|| {
            let mut e = ExplorationContextTool::new(session_id.clone());
            e.configure(&state.config.exploration, &state.config.context);
            Arc::new(e)
        }).clone()
    };

    let resp = match state.core.orchestrator.run(&body.question, "", ect).await {
        Ok(answer) => ChatResponse {
            code: 0, session_id, answer: Some(answer), error: None,
        },
        Err(e) => {
            let code = if e.contains("LLM") || e.contains("retry") { 3 }
                else if e.contains("context") || e.contains("ECT") { 4 }
                else { 5 };
            ChatResponse { code, session_id, answer: None, error: Some(e) }
        }
    };

    Json(resp)
}
