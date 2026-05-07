use explore_ai_agent::adapter::api_adapter::{LlmStructuredClient, UnifiedResponse};
use explore_ai_agent::agents::deep_explorer::CollectedEvidence;
use explore_ai_agent::agents::quality_evaluator::*;
use explore_ai_agent::context::exploration::{CriticalFile, ExplorationSummary};
use std::sync::Mutex;

// ============================================================================
// Mock LLM client — returns pre-configured responses for deterministic testing
// ============================================================================

struct MockLlmClient {
    /// The UnifiedResponse to return on the next call_llm_structured call.
    response: Mutex<Option<Result<UnifiedResponse, String>>>,
}

impl MockLlmClient {
    fn new() -> Self {
        MockLlmClient {
            response: Mutex::new(None),
        }
    }

    fn set_response(&self, response: Result<UnifiedResponse, String>) {
        *self.response.lock().unwrap() = Some(response);
    }
}

#[async_trait::async_trait]
impl LlmStructuredClient for MockLlmClient {
    async fn call_llm_structured(
        &self,
        _instructions: &str,
        _input_data: &serde_json::Value,
        _output_schema: Option<&serde_json::Value>,
    ) -> Result<UnifiedResponse, String> {
        self.response
            .lock()
            .unwrap()
            .take()
            .expect("MockLlmClient: call_llm_structured called without a preset response")
    }
}

// Helper: build a UnifiedResponse whose text field is the given JSON string.
fn mock_text_response(json: &str) -> Result<UnifiedResponse, String> {
    Ok(UnifiedResponse {
        text: Some(json.to_string()),
        tool_calls: vec![],
    })
}

// Helper: build a QualityEvaluation JSON string with the given confidence and action.
fn make_evaluation_json(confidence: f64, action: &str) -> String {
    format!(
        r#"{{
        "key_findings": "测试发现",
        "critical_files": [],
        "missing_info": "",
        "confidence": {},
        "action": "{}",
        "reason": "测试理由"
    }}"#,
        confidence, action
    )
}

// Helper: build a minimal QualityEvaluatorInput for tests.
fn make_evaluation_input() -> QualityEvaluatorInput {
    QualityEvaluatorInput {
        current_summary: ExplorationSummary {
            key_findings: "测试摘要".to_string(),
            critical_files: vec![],
            missing_info: String::new(),
            confidence: 0.5,
        },
        collected_evidence: vec![],
    }
}

// ============================================================================
// 8.2 数据结构测试 (QE-001 ~ QE-004d)
// ============================================================================

// QE-001: QualityEvaluation 序列化往返
#[test]
fn qe_001_quality_evaluation_roundtrip() {
    let original = QualityEvaluation {
        key_findings: "找到了核心校验逻辑".to_string(),
        critical_files: vec![QualityCriticalFile {
            path: "src/main.rs".to_string(),
            one_sentence_summary: "包含主函数入口".to_string(),
        }],
        missing_info: "缺少配置加载机制".to_string(),
        confidence: 0.85,
        action: ExplorationAction::Answer,
        reason: "核心逻辑已查明".to_string(),
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: QualityEvaluation = serde_json::from_str(&json).expect("反序列化失败");

    assert_eq!(restored.key_findings, original.key_findings);
    assert_eq!(restored.missing_info, original.missing_info);
    assert_eq!(restored.confidence, original.confidence);
    assert_eq!(restored.action, original.action);
    assert_eq!(restored.reason, original.reason);
    assert_eq!(restored.critical_files.len(), 1);
    assert_eq!(restored.critical_files[0].path, "src/main.rs");
    assert_eq!(
        restored.critical_files[0].one_sentence_summary,
        "包含主函数入口"
    );
}

// QE-002: ExplorationAction 枚举序列化
#[test]
fn qe_002_exploration_action_serialization() {
    let answer = serde_json::to_string(&ExplorationAction::Answer).expect("序列化失败");
    let deep_explore =
        serde_json::to_string(&ExplorationAction::DeepExplore).expect("序列化失败");

    assert_eq!(answer, "\"answer\"");
    assert_eq!(deep_explore, "\"deep_explore\"");
}

// QE-003: ExplorationAction 枚举反序列化
#[test]
fn qe_003_exploration_action_deserialization() {
    let answer: ExplorationAction = serde_json::from_str("\"answer\"").expect("反序列化失败");
    let deep_explore: ExplorationAction =
        serde_json::from_str("\"deep_explore\"").expect("反序列化失败");

    assert_eq!(answer, ExplorationAction::Answer);
    assert_eq!(deep_explore, ExplorationAction::DeepExplore);
}

// QE-004: ExplorationAction 非法值反序列化
#[test]
fn qe_004_exploration_action_invalid_value() {
    let result: Result<ExplorationAction, _> = serde_json::from_str("\"invalid_action\"");
    assert!(result.is_err(), "非法值反序列化应该失败");
}

// QE-004b: QualityCriticalFile 序列化往返
#[test]
fn qe_004b_quality_critical_file_roundtrip() {
    let original = QualityCriticalFile {
        path: "src/main.rs".to_string(),
        one_sentence_summary: "包含主函数".to_string(),
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: QualityCriticalFile = serde_json::from_str(&json).expect("反序列化失败");

    assert_eq!(restored.path, "src/main.rs");
    assert_eq!(restored.one_sentence_summary, "包含主函数");
}

// QE-004c: QualityEvaluatorInput 序列化往返（含证据）
#[test]
fn qe_004c_quality_evaluator_input_roundtrip_with_evidence() {
    let original = QualityEvaluatorInput {
        current_summary: ExplorationSummary {
            key_findings: "找到核心类".to_string(),
            critical_files: vec![CriticalFile {
                path: "src/validator.rs".to_string(),
                one_sentence_summary: "包含验证逻辑".to_string(),
            }],
            missing_info: "缺少配置信息".to_string(),
            confidence: 0.6,
        },
        collected_evidence: vec![
            CollectedEvidence {
                file: "src/validator.rs".to_string(),
                line: "42-47".to_string(),
                code_snippet: "if required && value == null {".to_string(),
                relevance: "required=true 时的校验逻辑".to_string(),
            },
            CollectedEvidence {
                file: "src/validator.rs".to_string(),
                line: "55-60".to_string(),
                code_snippet: "checkDefaultValue(value);".to_string(),
                relevance: "checkDefaultValue 调用".to_string(),
            },
        ],
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: QualityEvaluatorInput = serde_json::from_str(&json).expect("反序列化失败");

    assert_eq!(restored.current_summary.key_findings, "找到核心类");
    assert_eq!(restored.current_summary.confidence, 0.6);
    assert_eq!(restored.current_summary.critical_files.len(), 1);
    assert_eq!(restored.collected_evidence.len(), 2);
    assert_eq!(restored.collected_evidence[0].file, "src/validator.rs");
    assert_eq!(restored.collected_evidence[0].line, "42-47");
    assert_eq!(
        restored.collected_evidence[0].code_snippet,
        "if required && value == null {"
    );
}

// QE-004d: QualityEvaluatorInput 序列化往返（空证据）
#[test]
fn qe_004d_quality_evaluator_input_roundtrip_empty_evidence() {
    let original = QualityEvaluatorInput {
        current_summary: ExplorationSummary {
            key_findings: "发现部分相关文件".to_string(),
            critical_files: vec![],
            missing_info: "核心实现尚未定位".to_string(),
            confidence: 0.3,
        },
        collected_evidence: vec![],
    };

    let json = serde_json::to_string(&original).expect("序列化失败");
    let restored: QualityEvaluatorInput = serde_json::from_str(&json).expect("反序列化失败");

    assert!(restored.collected_evidence.is_empty());
    assert_eq!(
        restored.current_summary.key_findings,
        "发现部分相关文件"
    );
    assert_eq!(restored.current_summary.confidence, 0.3);
}

// ============================================================================
// 8.3 构造与 Schema 测试 (QE-007 ~ QE-012)
// ============================================================================

// QE-007: 构造评估专家
#[test]
fn qe_007_constructor_does_not_panic() {
    let qe = ExplorationQualityEvaluator::new();
    let _ = qe;
}

// QE-008: output_schema 返回合法 JSON
#[test]
fn qe_008_output_schema_returns_valid_json() {
    let schema_str = ExplorationQualityEvaluator::output_schema();
    let schema: serde_json::Value =
        serde_json::from_str(schema_str).expect("output_schema 不是合法的 JSON");

    assert!(schema.get("name").is_some(), "缺少 name 字段");
    assert!(schema.get("strict").is_some(), "缺少 strict 字段");
    assert!(schema.get("schema").is_some(), "缺少 schema 字段");
}

// QE-009: output_schema 的 name 字段
#[test]
fn qe_009_output_schema_name_field() {
    let schema_str = ExplorationQualityEvaluator::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    assert_eq!(
        schema["name"].as_str().unwrap(),
        "exploration_quality_evaluator_response"
    );
}

// QE-010: output_schema 含全部 6 个 required 字段
#[test]
fn qe_010_output_schema_has_all_required_fields() {
    let schema_str = ExplorationQualityEvaluator::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let required = schema["schema"]["required"]
        .as_array()
        .expect("required 应该是数组");

    let required_fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

    assert!(required_fields.contains(&"key_findings"));
    assert!(required_fields.contains(&"critical_files"));
    assert!(required_fields.contains(&"missing_info"));
    assert!(required_fields.contains(&"confidence"));
    assert!(required_fields.contains(&"action"));
    assert!(required_fields.contains(&"reason"));
}

// QE-011: output_schema 含 action 的 enum 约束
#[test]
fn qe_011_output_schema_action_enum_constraint() {
    let schema_str = ExplorationQualityEvaluator::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let action_enum = schema["schema"]["properties"]["action"]["enum"]
        .as_array()
        .expect("action.enum 应该是数组");

    let enum_values: Vec<&str> = action_enum.iter().filter_map(|v| v.as_str()).collect();

    assert!(enum_values.contains(&"answer"));
    assert!(enum_values.contains(&"deep_explore"));
    assert_eq!(enum_values.len(), 2);
}

// QE-012: output_schema 的 strict 为 true
#[test]
fn qe_012_output_schema_strict_is_true() {
    let schema_str = ExplorationQualityEvaluator::output_schema();
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    assert_eq!(schema["strict"].as_bool().unwrap(), true);
}

// ============================================================================
// 8.5 Prompt 组装测试 (QE-013 ~ QE-015)
// ============================================================================

// QE-013: assemble_instructions 返回的指令文本包含核心要素
#[test]
fn qe_013_assemble_instructions_contains_core_elements() {
    let instructions = ExplorationQualityEvaluator::assemble_instructions();
    assert!(
        instructions.contains("探索质量评估专家"),
        "指令文本应包含角色定义"
    );
    assert!(
        instructions.contains("工作流程"),
        "指令文本应包含工作流程说明"
    );
}

// QE-014: 指令文本包含置信度评分参考表
#[test]
fn qe_014_instructions_contains_confidence_table() {
    let instructions = ExplorationQualityEvaluator::assemble_instructions();
    assert!(
        instructions.contains("置信度"),
        "指令文本应包含置信度评分参考"
    );
    assert!(
        instructions.contains("0.8") || instructions.contains("1.0"),
        "指令文本应包含置信度数值参考"
    );
}

// QE-015: 指令文本包含示例输出
#[test]
fn qe_015_instructions_contains_example_output() {
    let instructions = ExplorationQualityEvaluator::assemble_instructions();
    assert!(
        instructions.contains("示例输出") || instructions.contains("示例"),
        "指令文本应包含示例输出章节"
    );
    assert!(
        instructions.contains("key_findings"),
        "指令文本应包含 JSON 字段示例"
    );
}

// ============================================================================
// 8.6 校验逻辑测试 (QE-026 ~ QE-032)
//
// 这些测试调用 evaluate()（设计文档 3.2.1 定义的公开入口）。
// MockLlmClient 注入预设的 LLM 响应，使 evaluate() 的完整流程
// "序列化 → 调适配层 → 反序列化 → 校验" 全部走通。
// ============================================================================

// QE-026: confidence = 0.0 合法
// 推导链：
//   QualityEvaluatorInput → 序列化 → mock.call_llm_structured()
//   → mock 返回 JSON {confidence:0.0, action:"answer"}
//   → 反序列化为 QualityEvaluation
//   → 校验: 0.0 ≤ 0.0 ≤ 1.0 → 通过 → 返回 Ok
#[tokio::test]
async fn qe_026_confidence_zero_is_valid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.0, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_ok(), "confidence=0.0 应校验通过");
}

// QE-027: confidence = 1.0 合法
#[tokio::test]
async fn qe_027_confidence_one_is_valid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(1.0, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_ok(), "confidence=1.0 应校验通过");
}

// QE-028: confidence = 0.5 合法
#[tokio::test]
async fn qe_028_confidence_half_is_valid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.5, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_ok(), "confidence=0.5 应校验通过");
}

// QE-029: confidence < 0.0 非法
// 推导链：
//   mock 返回 JSON {confidence:-0.1}
//   → 反序列化成功（JSON number 允许负数）
//   → 校验: -0.1 < 0.0 → 失败 → 返回 Err，含 "confidence out of range"
#[tokio::test]
async fn qe_029_confidence_negative_is_invalid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(-0.1, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_err(), "非法 confidence 应返回 Err");
    let err = result.unwrap_err();
    assert!(
        err.contains("confidence out of range"),
        "错误信息应包含 'confidence out of range'，实际: {}",
        err
    );
}

// QE-030: confidence > 1.0 非法
#[tokio::test]
async fn qe_030_confidence_above_one_is_invalid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(1.5, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_err(), "非法 confidence 应返回 Err");
    let err = result.unwrap_err();
    assert!(
        err.contains("confidence out of range"),
        "错误信息应包含 'confidence out of range'，实际: {}",
        err
    );
}

// QE-031: action = "answer" 合法
// 推导链：
//   mock 返回 JSON {action:"answer"}
//   → QualityEvaluation 反序列化 → action 解析为 ExplorationAction::Answer
//   → 校验通过 → 返回 Ok
#[tokio::test]
async fn qe_031_action_answer_is_valid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.8, "answer")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_ok(), "action='answer' 应校验通过");
    let eval = result.unwrap();
    assert_eq!(eval.action, ExplorationAction::Answer);
}

// QE-032: action = "deep_explore" 合法
#[tokio::test]
async fn qe_032_action_deep_explore_is_valid() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.3, "deep_explore")));

    let result = qe
        .evaluate("test question", &make_evaluation_input(), &mock)
        .await;
    assert!(result.is_ok(), "action='deep_explore' 应校验通过");
    let eval = result.unwrap();
    assert_eq!(eval.action, ExplorationAction::DeepExplore);
}

// ============================================================================
// 8.7 集成测试 (QE-033 ~ QE-035)
// ============================================================================

// QE-033: 快速探索后评估场景（collected_evidence 为空）
// 推导链：
//   QualityEvaluatorInput { collected_evidence: [] }
//   → evaluate() 序列化 → mock adapter.call_llm_structured()
//   → mock 返回预设 QualityEvaluation → 校验通过 → 返回 Ok
#[tokio::test]
async fn qe_033_fast_explore_evaluation_scenario() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.8, "answer")));

    let input = QualityEvaluatorInput {
        current_summary: ExplorationSummary {
            key_findings: "找到 BooleanValidator.java".to_string(),
            critical_files: vec![CriticalFile {
                path: "core/validation/BooleanValidator.java".to_string(),
                one_sentence_summary: "包含 BooleanValidator 类".to_string(),
            }],
            missing_info: "validate 方法的具体实现细节".to_string(),
            confidence: 0.6,
        },
        collected_evidence: vec![],
    };

    let result = qe
        .evaluate("BooleanValidator 有哪些配置参数？", &input, &mock)
        .await;
    assert!(result.is_ok(), "快速探索后评估应返回 Ok");
}

// QE-034: 深度探索后评估场景（collected_evidence 含证据）
#[tokio::test]
async fn qe_034_deep_explore_evaluation_scenario() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.85, "answer")));

    let input = QualityEvaluatorInput {
        current_summary: ExplorationSummary {
            key_findings: "找到 BooleanValidator.java 和 BooleanParam 注解定义".to_string(),
            critical_files: vec![CriticalFile {
                path: "core/validation/BooleanValidator.java".to_string(),
                one_sentence_summary: "包含 BooleanValidator 类".to_string(),
            }],
            missing_info: "validate 方法的具体实现细节".to_string(),
            confidence: 0.6,
        },
        collected_evidence: vec![
            CollectedEvidence {
                file: "core/validation/BooleanValidator.java".to_string(),
                line: "42-47".to_string(),
                code_snippet: "if (required && value == null) { throw new ValidationException(...); }"
                    .to_string(),
                relevance: "required=true 时的校验逻辑".to_string(),
            },
            CollectedEvidence {
                file: "core/validation/BooleanValidator.java".to_string(),
                line: "50-55".to_string(),
                code_snippet: "if (hasDefault && value.equals(defaultValue)) {".to_string(),
                relevance: "checkDefaultValue 调用入口".to_string(),
            },
            CollectedEvidence {
                file: "annotation/BooleanParam.java".to_string(),
                line: "15-20".to_string(),
                code_snippet: "@interface BooleanParam { boolean required() default true; }"
                    .to_string(),
                relevance: "注解定义含 required 和 defaultValue 属性".to_string(),
            },
        ],
    };

    let result = qe
        .evaluate("BooleanValidator 有哪些配置参数？", &input, &mock)
        .await;
    assert!(result.is_ok(), "深度探索后评估应返回 Ok");
}

// QE-035: evaluate 在序列化失败时返回 Err
// 推导链：
//   当 QualityEvaluatorInput 序列化失败时，evaluate() 步骤 1 应返回 Err，
//   错误信息含 "serialize"。当前所有字段均支持 serde 序列化，
//   此测试验证正常路径返回 Ok。
#[tokio::test]
async fn qe_035_evaluate_with_valid_input_returns_ok() {
    let qe = ExplorationQualityEvaluator::new();
    let mock = MockLlmClient::new();
    mock.set_response(mock_text_response(&make_evaluation_json(0.5, "answer")));

    let input = make_evaluation_input();
    let result = qe.evaluate("test question", &input, &mock).await;
    assert!(result.is_ok(), "正常输入应返回 Ok");
}
