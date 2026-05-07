use super::api_adapter::ApiAdapter;
use super::data_provider::DataProvider;
use super::types::*;

impl ApiAdapter {
    pub fn assemble_prompt(
        &self,
        template: &str,
        provider: &dyn DataProvider,
    ) -> String {
        let mut result = template.to_string();

        // {question}
        let question = provider.get_question();
        let chat_content = if question.is_empty() {
            String::new()
        } else {
            format!("## 用户问题\n{}", question)
        };
        result = self.replace_placeholder(&result, PLACEHOLDER_QUESTION, &chat_content);

        // {exploration_history}
        let history = provider.get_exploration_history();
        let chat_content = if history.is_null() {
            String::new()
        } else {
            let json_str = serde_json::to_string(&history).unwrap_or_default();
            if json_str.is_empty() || json_str == "null" {
                String::new()
            } else {
                format!("## 历史探索记录\n{}", json_str)
            }
        };
        result = self.replace_placeholder(&result, PLACEHOLDER_EXPLORATION_HISTORY, &chat_content);

        // {current_summary}
        let summary = provider.get_current_summary();
        let chat_content = {
            let json_str = serde_json::to_string(&summary).unwrap_or_default();
            if json_str.is_empty() || json_str == "null" {
                String::new()
            } else {
                format!("## 已有探索线索\n{}", json_str)
            }
        };
        result = self.replace_placeholder(&result, PLACEHOLDER_CURRENT_SUMMARY, &chat_content);

        // {tools}
        let tools = provider.get_tools();
        let chat_content = {
            let formatted = self.format_tools_for_chat_prompt(&tools);
            if formatted.trim().is_empty() {
                "## 可用工具\n".to_string()
            } else {
                format!("## 可用工具\n{}", formatted)
            }
        };
        result = self.replace_placeholder(&result, PLACEHOLDER_TOOLS, &chat_content);

        // {loop_warning}
        let warning = provider.get_loop_warning();
        let chat_content = match warning {
            Some(ref text) if !text.is_empty() => {
                format!("## ⚠️ 系统警告\n{}", text)
            }
            _ => String::new(),
        };
        result = self.replace_placeholder(&result, PLACEHOLDER_LOOP_WARNING, &chat_content);

        result
    }

    pub fn replace_placeholder(
        &self,
        template: &str,
        placeholder: &str,
        chat_content: &str,
    ) -> String {
        match self.api_mode {
            ApiMode::Chat => template.replace(placeholder, chat_content),
            ApiMode::Responses => template.replace(placeholder, ""),
        }
    }

    fn format_tools_for_chat_prompt(&self, tools: &[ToolDefinition]) -> String {
        let mut lines: Vec<String> = Vec::new();
        for tool in tools {
            lines.push(format!("### {}", tool.name));
            if !tool.description.is_empty() {
                lines.push(tool.description.clone());
            }
            let params_str = serde_json::to_string_pretty(&tool.parameters).unwrap_or_default();
            if !params_str.is_empty() && params_str != "null" {
                lines.push(format!("参数: {}", params_str));
            }
            lines.push(String::new());
        }
        lines.join("\n")
    }
}
