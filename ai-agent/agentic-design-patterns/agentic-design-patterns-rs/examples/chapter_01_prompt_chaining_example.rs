//! # Chapter 01: Prompt Chaining（提示词链）
//!
//! 本示例演示了 **Prompt Chaining** 设计模式：
//! 将一个复杂任务拆分为多个顺序执行的步骤，每一步的输出作为下一步的输入。
//!
//! ## 流程
//! 1. **提取 Agent** — 从非结构化文本中提取技术规格（自由文本）
//! 2. **转换 Agent** — 将提取结果转换为结构化的 JSON（Rust Struct）
//!
//! ## 为什么要分两步？
//! 虽然 Rig 的 `Extractor` 可以一步到位，但拆分为两步更好地体现了
//! Prompt Chaining 的核心思想：**分而治之，逐步精炼**。

use dotenvy::dotenv;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama;
use serde::{Deserialize, Serialize};
use std::error::Error;

/// 最终提取的技术规格。
///
/// 借助 `schemars::JsonSchema`，Rig 能自动生成 JSON Schema，
/// 引导 LLM 输出符合该结构的 JSON，并反序列化为此结构体。
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct Specifications {
    /// 处理器信息，如 "3.5 GHz octa-core"
    cpu: String,
    /// 内存信息，如 "16GB"
    memory: String,
    /// 存储信息，如 "1TB NVMe SSD"
    storage: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 从 .env 文件加载环境变量（如有需要）
    let _ = dotenv();

    // 初始化 Ollama 客户端（默认连接 http://localhost:11434）
    // Ollama 无需 API Key，故传入 `Nothing`
    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // ── Step 1: 提取 Agent ──────────────────────────────────────────
    // 从非结构化文本中提取技术规格，输出为自由文本。
    let extractor_agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "You are a technical analyst. \
             Extract only the technical specifications from the user's input.",
        )
        .temperature(0.0)
        .build();

    // ── Step 2: 转换 Agent（Extractor）────────────────────────────────
    // 接收自由文本，将其转换为符合 `Specifications` 结构的 JSON。
    let transformer = client
        .extractor::<Specifications>("qwen2.5:14b")
        .preamble(
            "You are a JSON formatting assistant. \
             Convert the provided specifications into the required JSON structure.",
        )
        .build();

    // ── 运行 Prompt Chain ───────────────────────────────────────────
    let input_text = "The new laptop model features a 3.5 GHz octa-core processor, 16GB of RAM, and a 1TB NVMe SSD.";

    println!("Running prompt chain with Rig...\n");

    // 第一步：调用提取 Agent
    println!("Step 1: Extracting specifications...");
    let extracted_text = extractor_agent.prompt(input_text).await?;
    println!("Extracted Text:\n{extracted_text}\n");

    // 第二步：调用转换 Extractor，自动解析为 Rust 结构体
    println!("Step 2: Transforming to structured JSON...");
    let specs = transformer.extract(&extracted_text).await?;

    println!("\n--- Final Structured Output ---");
    println!("{}", serde_json::to_string_pretty(&specs)?);

    Ok(())
}
