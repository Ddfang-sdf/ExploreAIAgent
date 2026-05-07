# Explore AI Agent

基于大语言模型的代码库智能探索代理。给定一个本地项目目录，Agent 自动搜索、阅读和分析代码，回答用户关于代码库的任何问题。

**核心思路**：快速探索（关键词搜索）定位关键文件 → 深度探索（LLM 自主调用工具）收集原始代码证据 → 质量评估 → 生成准确回答。全程自动，用户只需提问。

## 快速开始

### 环境要求

- Rust 1.80+
- DeepSeek API Key（[申请地址](https://platform.deepseek.com/)）

### 安装

```bash
git clone https://github.com/Ddfang-sdf/ExploreAIAgent.git
cd ExploreAIAgent
cargo build --release
```

### 配置

复制模板文件并填入你的 API Key：

```bash
cp config.template.yaml config.yaml
```

编辑 `config.yaml`：

```yaml
llm:
  api_mode: "chat"
  base_url: "https://api.deepseek.com/v1"
  api_key: "sk-your-key-here"
  model: "deepseek-v4-pro"
  max_retries: 3
  thinking: false

exploration:
  token_threshold: 12000
  max_fast_explore_rounds: 3
  early_termination_confidence: 0.8

workspace:
  path: "./workspace"
```

### 运行

将你要探索的项目放到 `workspace/` 目录下，然后：

```bash
cargo run --release
```

CLI 交互模式启动后，直接输入问题：

```
>>> 这个项目是做什么的？核心结构是什么样的？
>>> 回测功能是怎么验证策略效果的？
>>> /exit
```

## 工作原理

```
用户问题
  │
  ▼
SearchStrategyAgent（快速探索，最多 3 轮）
  │ LLM 设计关键词 → FastExplorer 批量搜索 → LLM 评估结果
  │
  ▼
ExplorationQualityEvaluator（质量评估）
  │ 评估探索数据是否足以回答问题
  │
  ├── 置信度高 → 直接回答
  │
  └── 置信度低 → DeepExplorer（深度探索，最多 75 次工具调用）
       │ LLM 自主决定调用 6 种只读工具
       │ search_content / search_files / read_file /
       │ list_dir / file_info / execute_shell
       │
       ▼
    MainAgent（基于全部探索证据生成回答）
```

## 可用工具

| 工具 | 功能 |
|------|------|
| `search_files` | 按 glob 模式搜索文件名 |
| `search_content` | 正则搜索文件内容 |
| `read_file` | 读取文件（支持行范围） |
| `list_dir` | 列出目录 |
| `file_info` | 文件元信息 + 代码统计 |
| `execute_shell` | 受限只读 Shell 命令 |
| `fast_explorer` | 批量关键词搜索（多关键词 OR） |

## 项目结构

```
src/
├── adapter/          # LLM API 适配层（Chat/Responses 双模式）
├── agents/           # 5 个 Agent + 2 个 Refiner
│   ├── search_strategy.rs    # 快速探索策略
│   ├── deep_explorer.rs      # 深度自主探索
│   ├── quality_evaluator.rs  # 探索质量评估
│   ├── main_agent.rs         # 最终回答生成
│   ├── exploration_refiner.rs # 探索上下文精炼
│   └── conversation_refiner.rs # 对话上下文精炼
├── common/           # 错误类型、配置管理、路径安全
├── context/          # 探索上下文与对话上下文存储
├── conversation/     # 多轮对话管理
├── fast_explorer/    # 批量关键词搜索引擎
├── ffi_bridge/       # C 语言 Shell 执行器 FFI
├── orchestrator/     # 核心流程编排
├── tools/            # 7 个底层只读工具
└── web/              # Web 服务（开发中）
```

## 配置说明

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `llm.api_mode` | `chat` | API 模式：`chat` 或 `responses` |
| `llm.base_url` | `https://api.deepseek.com/v1` | LLM API 地址 |
| `llm.model` | `deepseek-v4-pro` | 模型名称 |
| `llm.thinking` | `false` | 是否开启思考模式 |
| `exploration.max_fast_explore_rounds` | `3` | 快速探索最大轮次 |
| `exploration.token_threshold` | `12000` | 探索上下文精炼触发阈值 |
| `workspace.path` | `./workspace` | 待探索项目目录 |

## 许可证

MIT
