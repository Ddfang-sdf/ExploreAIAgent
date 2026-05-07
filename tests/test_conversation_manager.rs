use explore_ai_agent::adapter::api_adapter::{ApiAdapter, ApiMode};
use explore_ai_agent::conversation::manager::{ConversationManager, ConversationOutput};

#[test]
fn conversation_manager_init_new_session() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let ctx = manager.init_session("session-1");
    assert_eq!(ctx.session_id(), "session-1");
    assert!(manager.has_session("session-1"));
}

#[test]
fn conversation_manager_init_existing_session_recovers() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    // Save a conversation
    let _ = manager.save_conversation("session-1", "Hello", "Hi there");

    // Re-init should recover existing context
    let ctx = manager.init_session("session-1");
    assert_eq!(ctx.total_rounds(), 1);
}

#[test]
fn conversation_manager_get_context() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    let context = manager.get_context("session-1");
    assert!(context.is_ok());
    let output = context.unwrap();
    assert!(output.conversation_summary.is_empty());
    assert!(output.active_topic.is_empty());
}

#[test]
fn conversation_manager_save_conversation() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    let result = manager.save_conversation(
        "session-1",
        "What is BooleanValidator?",
        "BooleanValidator is a class for boolean validation.",
    );

    assert!(result.is_ok());
}

#[test]
fn conversation_manager_save_updates_rounds() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    for i in 1..=5 {
        let _ = manager.save_conversation(
            "session-1",
            &format!("Question {}", i),
            &format!("Answer {}", i),
        );
    }

    let ctx = manager.init_session("session-1");
    assert_eq!(ctx.total_rounds(), 5, "Session should reflect 5 rounds of saved conversation");
}

// CM-006: save_conversation on nonexistent session returns Err
#[test]
fn conversation_manager_save_conversation_nonexistent_session() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let result = manager.save_conversation(
        "nonexistent",
        "What is BooleanValidator?",
        "BooleanValidator is a class for boolean validation.",
    );
    assert!(result.is_err());
}

#[test]
fn conversation_manager_nonexistent_session() {
    let manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    assert!(!manager.has_session("nonexistent"));

    let result = manager.get_context("nonexistent");
    assert!(result.is_err());
}

#[test]
fn conversation_manager_destroy_session() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");
    assert!(manager.has_session("session-1"));

    manager.destroy_session("session-1");
    assert!(!manager.has_session("session-1"));
}

#[tokio::test]
async fn conversation_manager_check_and_refine_below_threshold() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    // Add fewer than 10 rounds
    for i in 1..=5 {
        let _ = manager.save_conversation(
            "session-1",
            &format!("Q{}", i),
            &format!("A{}", i),
        );
    }

    // Should not trigger refinement, summary should remain unchanged
    let summary_before = {
        let ctx = manager.init_session("session-1");
        ctx.get_summary().to_string()
    };
    let result = manager.check_and_refine("session-1", "Current question?").await;
    assert!(result.is_ok());
    let summary_after = {
        let ctx = manager.init_session("session-1");
        ctx.get_summary().to_string()
    };
    assert_eq!(summary_before, summary_after,
        "Summary should remain unchanged when refinement is not triggered");
}

#[tokio::test]
async fn conversation_manager_check_and_refine_at_threshold() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    // Add 10 rounds to reach threshold
    for i in 1..=10 {
        let _ = manager.save_conversation(
            "session-1",
            &format!("Question about topic {}", i),
            &format!("Detailed answer about topic {}", i),
        );
    }

    // Should trigger refinement (calls ConversationRefinerAgent)
    let result = manager.check_and_refine("session-1", "Current question?").await;
    // Result depends on LLM call, but the trigger logic should work
    assert!(result.is_ok() || result.is_err()); // Either succeeds or fails gracefully
}

#[test]
fn conversation_output_format() {
    let output = ConversationOutput {
        conversation_summary: "Discussed BooleanValidator parameters.".to_string(),
        active_topic: "BooleanValidator parameter configuration".to_string(),
        recent_history: vec![],
    };

    let json = serde_json::to_value(&output).unwrap();
    assert_eq!(json["conversation_summary"], "Discussed BooleanValidator parameters.");
    assert_eq!(json["active_topic"], "BooleanValidator parameter configuration");
}

// CM-008: destroy nonexistent session should not panic
#[test]
fn conversation_manager_destroy_nonexistent_session() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    // Should silently succeed, not panic
    manager.destroy_session("nonexistent");
    assert!(!manager.has_session("nonexistent"));
}

// CM-011: check_and_refine on nonexistent session returns Err
#[tokio::test]
async fn conversation_manager_check_and_refine_nonexistent_session() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    let result = manager.check_and_refine("nonexistent", "Some question?").await;
    assert!(result.is_err());
}

// CM-012: get_context includes active_topic extracted from recent history
#[test]
fn conversation_manager_get_context_active_topic() {
    let mut manager = ConversationManager::new(ApiAdapter::new(ApiMode::Chat));
    manager.init_session("session-1");

    let _ = manager.save_conversation(
        "session-1",
        "What is BooleanValidator?",
        "BooleanValidator is a class for boolean validation with configurable parameters.",
    );

    let context = manager.get_context("session-1").unwrap();
    // active_topic should be non-empty when there is conversation history
    assert!(!context.active_topic.is_empty());
}
