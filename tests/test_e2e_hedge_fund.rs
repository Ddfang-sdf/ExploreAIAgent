use explore_ai_agent::cli::assemble_core;
use explore_ai_agent::common::config::AppConfig;
use explore_ai_agent::context::exploration::ExplorationContextTool;

const ROUNDS: &[(&str, &str)] = &[
    (
        "round1",
        "这个项目是做什么的？核心结构是什么样的？",
    ),
    (
        "round2",
        "巴菲特的真实出生地是哪里？",
    ),
    (
        "round3",
        "那巴菲特分析师的选股逻辑具体是怎么实现的？",
    ),
    (
        "round4",
        "如果要添加一个新的分析大师（比如索罗斯），需要修改哪些文件？",
    ),
    (
        "round5",
        "这个项目支持美股实时行情吗？",
    ),
    (
        "round6",
        "前面提的巴菲特分析师和芒格分析师，它们的Prompt有什么不同？",
    ),
    (
        "round7",
        "这个项目的回测功能是怎么验证策略效果的？",
    ),
];

#[tokio::test]
async fn e2e_hedge_fund() {
    let config = AppConfig::load(Some("config.yaml")).expect("加载配置失败");

    println!("=== AI Hedge Fund E2E Test ===\n");

    let core = assemble_core(&config).expect("模块初始化失败");
    let mut ect = ExplorationContextTool::new("hedge-fund-e2e".to_string());
    ect.configure(&config.exploration, &config.context);

    for (round_name, question) in ROUNDS {
        println!("========================================");
        println!("【{}】: {}", round_name, question);
        println!("========================================");

        match core.orchestrator.run(question, &mut ect).await {
            Ok(answer) => {
                println!("回答:\n{}\n", answer);
            }
            Err(e) => {
                println!("[ERROR] {}\n", e);
            }
        }
    }

    println!("=== E2E Test Complete ===");
}
