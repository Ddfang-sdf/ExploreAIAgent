# Explore AI Agent

基于 LLM 的代码库智能探索代理。给定一个本地项目目录，Agent 自主搜索、阅读和分析代码，回答关于代码库的任何问题。

**核心**：MainAgent 通过 function-calling 调用 8 个工具探索代码库，SSE 流式输出思考过程，OpenCode 风格上下文精炼。

**支持 OpenAI Chat Completions 和 Anthropic Messages 两种 API 协议**，一个 `api_protocol` 配置切换。

## 快速开始

### 环境要求

- Rust 1.80+ （需 MSVC 工具链用于编译 C 模块）
- LLM API Key（支持 MiniMax / DeepSeek 等兼容模型）
- 支持 OpenAI 和 Anthropic 两种 API 协议

### 安装

```bash
git clone https://github.com/Ddfang-sdf/ExploreAIAgent.git
cd ExploreAIAgent
cargo build --release
```

### 配置

```bash
cp config.template.yaml config.yaml
```

编辑 `config.yaml`：

```yaml
# === OpenAI 协议（默认）===
llm:
  api_key: "sk-your-key"
  base_url: "https://api.minimaxi.com/v1"
  model: "MiniMax-M2.7"
  thinking: true

# === 或 Anthropic 协议 ===
# llm:
#   api_key: "sk-your-key"
#   base_url: "https://api.minimaxi.com/anthropic"
#   model: "MiniMax-M2.7"
#   api_protocol: "anthropic"

exploration:
  compact_token_threshold: 10000

deep_explorer:
  enable: true
  max_tool_calls: 75

fast_explore:
  enable: true

workspace:
  path: "./workspace"
```

### 运行

```bash
# CLI 模式
cargo run --release

# Web 模式（axum HTTP 服务）
cargo run --release -- --web
# 服务启动在 http://localhost:3000
```

CLI 交互：

```
>>> 这个项目是做什么的？
>>> 回测功能怎么实现？
>>> /exit
```

## Web API

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/chat` | POST | 单次问答（JSON 响应） |
| `/chat/stream` | POST | 流式问答（SSE，实时推送思考过程） |

请求格式：

```json
{"question": "项目怎么回测？", "session_id": "可选"}
```

## 架构

```
MainAgent（决策中心，LLM function-calling 自主选择工具）
  ├── search_content    # 正则搜索文件内容
  ├── search_files      # Glob 文件名搜索
  ├── read_file         # 读取文件内容
  ├── list_dir          # 列出目录
  ├── file_info         # 文件元信息
  ├── execute_shell     # 只读 Shell
  ├── fast_explore      # 关键词批量搜索（可选）
  └── deep_explore      # 子代理深度探索（可选）
       │
       ▼
  基于探索数据生成最终答案
```

**模型适配层**：`ModelAdapter` trait 统一 OpenAI / Anthropic 两种 API 协议。消息格式转换、工具调用、SSE 流式解析全部封装在适配层内，业务代码零感知。

**思考过程**：SSE 流式解析 `reasoning_details` / `reasoning` / `reasoning_text` / `<think>` 标签，实时展示。

**上下文精炼**：OpenCode SUMMARY_TEMPLATE 原版模板，工具输出 2000 字符截断，assistant/tool 配对保护。

## 配置说明

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `llm.api_key` | — | **必填** |
| `llm.base_url` | `https://api.deepseek.com/v1` | API 地址 |
| `llm.model` | `deepseek-chat` | 模型名称 |
| `llm.thinking` | `false` | MiniMax 思考模式 |
| `llm.api_protocol` | `openai` | API 协议：`openai` 或 `anthropic` |
| `exploration.compact_token_threshold` | `8000` | 压缩触发 token 数 |
| `deep_explorer.enable` | `true` | 是否启用深度探索 |
| `deep_explorer.max_tool_calls` | `75` | DE 最大工具调用次数 |
| `fast_explore.enable` | `true` | 是否启用快速扫描 |
| `workspace.path` | `./workspace` | 待探索项目目录 |

## 项目结构

```
src/
├── adapter/
│   ├── model/          # 模型适配层（OpenAI Chat / Anthropic Messages）
│   ├── api_adapter.rs  # LLM API 客户端 + Trait
│   ├── retry.rs        # HTTP 重试（OpenCode 风格指数退避 + 抖动）
│   ├── response.rs     # 响应解析
│   ├── reasoning.rs    # 思考内容提取链
│   ├── protocol.rs     # 工具结果消息构建
│   └── types.rs        # 统一类型定义
├── agents/             # Agent（MainAgent / DE / Compactor）
├── common/             # 配置管理、错误类型、路径安全、截断控制
├── context/            # 探索/对话上下文存储
├── conversation/       # 多轮对话管理
├── ffi_bridge/         # C 语言 Shell 执行器 FFI
├── orchestrator/       # 模块组装 + ToolDispatcher
├── tools/              # 底层只读工具 + Shell 安全
└── web/                # Web API
csrc/                   # C 层（execute_shell 核心引擎）
```

## 许可证

MIT
