use std::io;
use std::sync::Arc;

use axum::{Router, routing::{get, post}, extract::State, Json, response::sse::{Event, Sse}};
use tower_http::cors::{CorsLayer, Any};
use std::convert::Infallible;
use tokio_stream::StreamExt;
use explore_ai_agent::agents::main_agent::{self, SseEvent};
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
    core: Arc<cli::CoreModules>,
    sessions: Arc<Mutex<HashMap<String, Arc<ExplorationContextTool>>>>,
    config: AppConfig,
}

async fn run_web(config: AppConfig) -> Result<(), String> {
    let core = cli::assemble_core(&config)?;
    let state = Arc::new(WebState {
        core: Arc::new(core),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        config,
    });

    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);
    let app = Router::new()
        .route("/health", get(health))
        .route("/chat", post(chat))
        .route("/chat/stream", post(chat_stream))
        .layer(cors)
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

async fn chat_stream(
    State(state): State<Arc<WebState>>,
    Json(body): Json<ChatRequest>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let session_id = body.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());
    let ect = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions.entry(session_id.clone()).or_insert_with(|| {
            let mut e = ExplorationContextTool::new(session_id.clone());
            e.configure(&state.config.exploration, &state.config.context);
            Arc::new(e)
        }).clone()
    };

    let rx = main_agent::sse_enable();
    let q = body.question.clone();
    let core = state.core.clone();
    let sid = session_id.clone();

    tokio::spawn(async move {
        let result = core.orchestrator.run(&q, "", ect).await;
        let tx_opt = main_agent::SSE_TX.lock().unwrap().clone();
        if let Some(tx) = tx_opt {
            match result {
                Ok(answer) => {
                    let _ = tx.send(SseEvent::Answer(answer));
                }
                Err(e) => {
                    let _ = tx.send(SseEvent::Answer(format!("错误: {}", e)));
                }
            }
            let _ = tx.send(SseEvent::Done);
        }
        main_agent::sse_disable();
    });

    use tokio_stream::StreamExt;
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(move |e| {
        let data = match e {
            SseEvent::Thinking(t) => serde_json::json!({"type":"thinking","text":t}).to_string(),
            SseEvent::Answer(t) => serde_json::json!({"type":"answer","text":t}).to_string(),
            SseEvent::Done => serde_json::json!({"type":"done","session_id":sid}).to_string(),
        };
        Ok(Event::default().data(data))
    });
    Sse::new(stream)
}
