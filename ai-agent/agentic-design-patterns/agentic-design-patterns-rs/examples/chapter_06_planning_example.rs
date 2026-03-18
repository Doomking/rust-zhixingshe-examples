use rig::{
    client::{CompletionClient, Nothing},
    completion::Prompt,
    providers::ollama,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// 扩展：使用结构化输出定义规划结果，让大模型必须遵循此结构
// 通过这种方式，我们将规划(Planning)的职责独立出来，并与执行(Execution)分离
#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
struct Outline {
    /// 核心论点
    core_thesis: String,
    /// 文章大纲的各个章节和小节（要点）
    sections: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // 加载环境变量
    dotenvy::dotenv().ok();

    // 1. 明确指定使用的模型
    // 使用 Ollama API
    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // 使用 qwen2.5:14b 作为默认模型
    let model = "qwen2.5:14b";

    // 2. 针对规划模式的扩展：分离职责，定义两个明确且聚焦的智能体

    // Planner Agent: 负责全局规划和生成大纲
    let planner = client.extractor::<Outline>(model)
        .preamble("你是一位专业的内容策略师和规划师。\
                  你的专长在于为技术文章创建清晰、可操作且符合逻辑的结构。\
                  你总是能将复杂的主题分解为易于理解的章节。")
        .build();

    // Writer Agent: 负责根据已有的大纲执行具体的撰写任务
    let writer = client.agent(model)
        .preamble("你是一位专业的技术作家。\
                  你的任务是严格根据提供的大纲撰写引人入胜、简明扼要且信息丰富的摘要。\
                  请保持专业且通俗易懂的语气。")
        .build();

    // 3. 定义任务目标
    let topic = "强化学习在人工智能中的重要性";

    // 开始执行: 步骤 1 - 规划阶段
    println!("## 第一步：运行规划智能体(Planner Agent) ##");
    let plan_prompt = format!(
        "请为目标主题创建一个结构化的摘要大纲：'{}'",
        topic
    );

    // 使用提取器(extractor)获取结构化的规划结果
    println!("正在思考与规划中...");
    let outline = planner.extract(&plan_prompt).await?;

    println!("\n--- 生成的大纲 ---");
    println!("核心论点: {}", outline.core_thesis);
    println!("各章节/要点:");
    for (i, section) in outline.sections.iter().enumerate() {
        println!("  {}. {}", i + 1, section);
    }

    // 开始执行: 步骤 2 - 撰写阶段
    println!("\n\n## 第二步：基于规划大纲运行撰写智能体(Writer Agent) ##");
    let write_prompt = format!(
        "请针对主题写一篇200字左右的摘要：'{}'。\n\
         你必须严格遵循以下具体的大纲：\n\n\
         核心论点: {}\n\
         各章节/要点:\n{}",
        topic,
        outline.core_thesis,
        outline
            .sections
            .iter()
            .map(|s| format!("- {}", s))
            .collect::<Vec<_>>()
            .join("\n")
    );

    println!("正在根据大纲撰写草稿...");
    let final_summary = writer.prompt(&write_prompt).await?;

    println!("\n\n---\n## 最终任务结果 ##\n---");
    println!("### 摘要\n{}", final_summary);

    Ok(())
}
