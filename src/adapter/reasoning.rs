use serde_json::Value;

/// Extract thinking/reasoning content from raw LLM response.
/// Each implementation detects one provider-agnostic pattern.
pub trait ReasoningExtractor: Send + Sync {
    /// Returns Some(reasoning_text) if this extractor matches the response.
    /// Must NOT mutate `raw`.
    fn extract(&self, raw: &Value) -> Option<String>;

    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// Individual extractors — each is a self-contained pattern detector
// ---------------------------------------------------------------------------

/// DeepSeek R1 / V3: `choices[0].message.reasoning_content`
pub struct ReasoningContentExtractor;

impl ReasoningExtractor for ReasoningContentExtractor {
    fn extract(&self, raw: &Value) -> Option<String> {
        let content = raw
            .get("choices")?
            .get(0)?
            .get("message")?
            .get("reasoning_content")?
            .as_str()?;
        if content.is_empty() {
            None
        } else {
            Some(content.to_string())
        }
    }

    fn name(&self) -> &'static str {
        "reasoning_content"
    }
}

/// MiniMax-2.7 with `reasoning_split: true`: `choices[0].message.reasoning_details`
pub struct ReasoningDetailsExtractor;

impl ReasoningExtractor for ReasoningDetailsExtractor {
    fn extract(&self, raw: &Value) -> Option<String> {
        let details = raw
            .get("choices")?
            .get(0)?
            .get("message")?
            .get("reasoning_details")?
            .as_array()?;
        let mut parts: Vec<String> = Vec::new();
        for item in details {
            let t = item.get("type")?.as_str()?;
            if t == "reasoning.text" {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    fn name(&self) -> &'static str {
        "reasoning_details"
    }
}

/// MiniMax-2.7 default: `<think>...</think>` embedded in `content`
pub struct ThinkTagExtractor;

impl ReasoningExtractor for ThinkTagExtractor {
    fn extract(&self, _raw: &Value) -> Option<String> {
        // This extractor works on content, not raw — see strip_think_tag
        None
    }

    fn name(&self) -> &'static str {
        "think_tag"
    }
}

impl ThinkTagExtractor {
    /// Strip `<think>...</think>` from content text, return (reasoning, cleaned_content).
    /// Called separately because it mutates the content string.
    pub fn strip(text: &str) -> (Option<String>, String) {
        if let Some(start) = text.find("<think>") {
            let content_start = start + 7; // "<think>".len()
            if let Some(end) = text[content_start..].find("</think>") {
                let reasoning = text[content_start..content_start + end].to_string();
                let before = &text[..start];
                let after = &text[content_start + end + 8..]; // 8 = "</think>".len()
                let cleaned = format!("{}{}", before.trim(), after.trim());
                return (Some(reasoning), cleaned);
            }
        }
        (None, text.to_string())
    }
}

// ---------------------------------------------------------------------------
// Chain
// ---------------------------------------------------------------------------

pub struct ReasoningChain {
    extractors: Vec<Box<dyn ReasoningExtractor>>,
}

impl ReasoningChain {
    /// Create chain with all built-in extractors.
    pub fn default_chain() -> Self {
        let mut chain = Self { extractors: Vec::new() };
        chain.register(ReasoningContentExtractor);
        chain.register(ReasoningDetailsExtractor);
        chain.register(ThinkTagExtractor);
        chain
    }

    pub fn register(&mut self, e: impl ReasoningExtractor + 'static) {
        self.extractors.push(Box::new(e));
    }

    /// Try each extractor in registration order; first match wins.
    /// `content_text` is the already-parsed content string (for ThinkTag).
    pub fn extract(&self, raw: &Value, content_text: Option<&str>) -> (Option<String>, Option<String>) {
        // 1. Try the raw-response extractors first
        for e in &self.extractors {
            if e.name() == "think_tag" {
                continue; // handled below
            }
            if let Some(r) = e.extract(raw) {
                return (Some(r), content_text.map(|s| s.to_string()));
            }
        }
        // 2. Fall back to <think> tag stripping inside content
        if let Some(text) = content_text {
            if !text.is_empty() {
                let (reasoning, cleaned) = ThinkTagExtractor::strip(text);
                if reasoning.is_some() {
                    return (reasoning, Some(cleaned));
                }
                return (None, Some(text.to_string()));
            }
        }
        (None, content_text.map(|s| s.to_string()))
    }
}
