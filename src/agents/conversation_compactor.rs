use crate::adapter::api_adapter::LlmToolClient;

/// OpenCode's SUMMARY_TEMPLATE, verbatim.
const SUMMARY_TEMPLATE: &str = concat!(
    "Output exactly the Markdown structure shown inside <template> and keep the section order unchanged. Do not include the <template> tags in your response.\n",
    "<template>\n",
    "## Goal\n",
    "- [single-sentence task summary]\n",
    "\n",
    "## Constraints & Preferences\n",
    "- [user constraints, preferences, specs, or \"(none)\"]\n",
    "\n",
    "## Progress\n",
    "### Done\n",
    "- [completed work or \"(none)\"]\n",
    "\n",
    "### In Progress\n",
    "- [current work or \"(none)\"]\n",
    "\n",
    "### Blocked\n",
    "- [blockers or \"(none)\"]\n",
    "\n",
    "## Key Decisions\n",
    "- [decision and why, or \"(none)\"]\n",
    "\n",
    "## Next Steps\n",
    "- [ordered next actions or \"(none)\"]\n",
    "\n",
    "## Critical Context\n",
    "- [important technical facts, errors, open questions, or \"(none)\"]\n",
    "\n",
    "## Relevant Files\n",
    "- [file or directory path: why it matters, or \"(none)\"]\n",
    "</template>\n",
    "\n",
    "Rules:\n",
    "- Keep every section, even when empty.\n",
    "- Use terse bullets, not prose paragraphs.\n",
    "- Preserve exact file paths, commands, error strings, and identifiers when known.\n",
    "- Do not mention the summary process or that context was compacted.",
);

/// Truncate tool output to this many characters before sending to the compact LLM.
const TOOL_OUTPUT_MAX_CHARS: usize = 2000;

pub struct ConversationCompactor;

impl ConversationCompactor {
    pub fn new() -> Self {
        ConversationCompactor
    }

    fn build_prompt(previous_summary: Option<&str>, context: &str) -> String {
        let anchor = if let Some(prev) = previous_summary {
            format!(
                "Update the anchored summary below using the conversation history above.\n\
                 Preserve still-true details, remove stale details, and merge in the new facts.\n\
                 <previous-summary>\n{}\n</previous-summary>",
                prev
            )
        } else {
            "Create a new anchored summary from the conversation history above.".to_string()
        };
        format!("{}\n\n{}\n\n{}", anchor, SUMMARY_TEMPLATE, context)
    }

    /// Compact conversation history. Messages are truncated before compaction
    /// to keep the compact call itself small and fast (OpenCode-style).
    pub async fn compact(
        &self,
        older_messages: &[serde_json::Value],
        previous_summary: Option<&str>,
        client: &dyn LlmToolClient,
    ) -> Result<String, String> {
        // Truncate tool outputs to TOOL_OUTPUT_MAX_CHARS like OpenCode does.
        // This prevents large file contents from bloating the compact request.
        let truncated: Vec<String> = older_messages.iter().map(|msg| {
            let content = msg.get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "tool" && content.len() > TOOL_OUTPUT_MAX_CHARS {
                format!("[{}: {}] {}...(truncated)",
                    role,
                    msg.get("tool_call_id").and_then(|i| i.as_str()).unwrap_or(""),
                    &content[..TOOL_OUTPUT_MAX_CHARS])
            } else if content.len() > TOOL_OUTPUT_MAX_CHARS {
                format!("[{}] {}...(truncated)", role, &content[..TOOL_OUTPUT_MAX_CHARS])
            } else {
                format!("[{}] {}", role, content)
            }
        }).collect();

        let context = truncated.join("\n\n");
        let prompt = Self::build_prompt(previous_summary, &context);

        // MiniMax requires at least one user message; wrap in system+user
        let compact_messages = vec![
            serde_json::json!({"role": "system", "content": prompt}),
            serde_json::json!({"role": "user", "content": context}),
        ];
        let response = client
            .call_llm_with_tools(&compact_messages, &[], None)
            .await?;

        response.text.as_deref().map(|t| t.to_string()).ok_or_else(|| "empty compact response".to_string())
    }
}
