use explore_ai_agent::context::conversation::*;

#[test]
fn conversation_context_new_session() {
    let ctx = ConversationContextTool::new("session-1".to_string());
    assert_eq!(ctx.session_id(), "session-1");
    assert_eq!(ctx.total_rounds(), 0);
    assert_eq!(ctx.summarized_rounds(), 0);
    assert!(ctx.get_history().is_empty());
    assert!(ctx.get_summary().is_empty());
}

#[test]
fn conversation_context_add_record() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    ctx.add_record(
        "What is BooleanValidator?".to_string(),
        "BooleanValidator is a class for boolean validation.".to_string(),
    );
    assert_eq!(ctx.total_rounds(), 1);
    assert_eq!(ctx.get_history().len(), 1);

    let record = &ctx.get_history()[0];
    assert_eq!(record.round, 1);
    assert_eq!(record.user_question, "What is BooleanValidator?");
}

#[test]
fn conversation_context_multiple_rounds() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    for i in 1..=5 {
        ctx.add_record(
            format!("Question {}", i),
            format!("Answer {}", i),
        );
    }
    assert_eq!(ctx.total_rounds(), 5);
    assert_eq!(ctx.get_history().len(), 5);

    // Round numbers should be sequential
    for (idx, record) in ctx.get_history().iter().enumerate() {
        assert_eq!(record.round, (idx + 1) as u32);
    }
}

#[test]
fn conversation_context_get_recent_history() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    for i in 1..=15 {
        ctx.add_record(
            format!("Question {}", i),
            format!("Answer {}", i),
        );
    }

    let recent = ctx.get_recent_history(10);
    assert_eq!(recent.len(), 10);
    // Should be the most recent 10 records
    assert_eq!(recent[0].round, 6);
    assert_eq!(recent[9].round, 15);
}

#[test]
fn conversation_context_update_summary() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    ctx.update_summary("Summary of first 10 rounds.".to_string());
    assert_eq!(ctx.get_summary(), "Summary of first 10 rounds.");
}

#[test]
fn conversation_context_needs_refinement_by_rounds() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());

    // Below threshold
    for i in 1..=9 {
        ctx.add_record(format!("Q{}", i), format!("A{}", i));
    }
    assert!(!ctx.needs_refinement());

    // At threshold
    ctx.add_record("Q10".to_string(), "A10".to_string());
    assert!(ctx.needs_refinement());
}

#[test]
fn conversation_context_needs_refinement_by_tokens() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());

    // 3 records × ~2500 chars per record ≈ 7500 bytes + JSON overhead
    // → > 8000 bytes → > 2000 tokens, exceeding CONVERSATION_TOKEN_THRESHOLD.
    for _i in 1..=3 {
        ctx.add_record(
            "x".repeat(1350),
            "y".repeat(1350),
        );
    }
    assert!(
        ctx.needs_refinement(),
        "Should need refinement when token count exceeds CONVERSATION_TOKEN_THRESHOLD (2000)"
    );
}

#[test]
fn conversation_context_serialization() {
    let ctx_data = ConversationContext {
        session_id: "test-session".to_string(),
        conversation_history: vec![ConversationRecord {
            round: 1,
            user_question: "Hello?".to_string(),
            answer_summary: "Hi!".to_string(),
            timestamp: chrono::Utc::now(),
        }],
        conversation_summary: "Discussed greetings".to_string(),
        metadata: ConversationMetadata {
            total_rounds: 1,
            summarized_rounds: 0,
            last_updated: chrono::Utc::now(),
        },
    };

    let json = serde_json::to_value(&ctx_data).unwrap();
    assert_eq!(json["session_id"], "test-session");
    assert!(json["conversation_history"].is_array());
    assert_eq!(json["conversation_history"][0]["round"], 1);

    let deserialized: ConversationContext = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.session_id, "test-session");
    assert_eq!(deserialized.conversation_history.len(), 1);
}

// CCT-005: get_recent_history returns all records when n > history.len()
#[test]
fn conversation_context_get_recent_history_n_gt_history_len() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    for i in 1..=3 {
        ctx.add_record(
            format!("Question {}", i),
            format!("Answer {}", i),
        );
    }

    // n (10) > history.len() (3) → return all 3 records
    let recent = ctx.get_recent_history(10);
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0].round, 1);
    assert_eq!(recent[2].round, 3);
}

// CCT-010: update_summary sets summarized_rounds to total_rounds
#[test]
fn conversation_context_update_summary_updates_summarized_rounds() {
    let mut ctx = ConversationContextTool::new("session-1".to_string());
    for i in 1..=10 {
        ctx.add_record(format!("Q{}", i), format!("A{}", i));
    }
    assert_eq!(ctx.total_rounds(), 10);
    assert_eq!(ctx.summarized_rounds(), 0);

    ctx.update_summary("Compressed summary of first 10 rounds.".to_string());
    assert_eq!(ctx.summarized_rounds(), 10);
}
