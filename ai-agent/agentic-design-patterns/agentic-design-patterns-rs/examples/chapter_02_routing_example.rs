//! # Chapter 02: Routing (路由模式)
//!
//! 本示例演示了 **Routing** 设计模式：
//! 根据用户的请求意图，将任务分发（路由）到专门处理该任务的管道，并在最终处理节点通过**函数调用 (Function Calling / Tools)** 执行具体操作。
//!
//! 本代码实现参考了 rig 官方的 Semantic Router (pipeline) 模式：
//! 将意图分类的大模型输出映射为新的 Prompt，然后发送给带有多工具 (Tools) 装备的通用大模型。
//!
//! ## 核心流程
//! 1. **定义工具集 (Tools)** — `BookingTool` 和 `InfoTool`，它们具备真正的功能，LLM 可以通过提供标准化 JSON 实参来调用（等同于 Python ADK 的 tools）。
//! 2. **分类器智能体 (Classifier Agent)** — 仅输出限定的主题（booker, info, unclear）。
//! 3. **处理管道 (Pipeline Router)** — `pipeline::new()` 将分类器、动态 Prompt 重写、和最终自带多工具的智能体串联。如果在 Pipeline 中分类到了具体意图，最终的 prompt 就会指令大模型去调用其携带的正确工具。

use dotenvy::dotenv;
use rig::client::{CompletionClient, Nothing};
use rig::completion::ToolDefinition;
use rig::pipeline::{self, Op, TryOp};
use rig::providers::ollama;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::error::Error;

// ── 1. 定义工具 (Tools) ──────────────────────────────────────────────
// 等同于 ADK 中传递给 specialized agents 的 tools

#[derive(Deserialize, Serialize)]
struct BookingArgs {
    request: String,
}

#[derive(Debug)]
struct ToolError(String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolError: {}", self.0)
    }
}

impl std::error::Error for ToolError {}

#[derive(Deserialize, Serialize)]
struct BookingTool;

impl Tool for BookingTool {
    const NAME: &'static str = "booker";
    type Error = ToolError;
    type Args = BookingArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "booker",
            "description": "Book a flight or hotel",
            "parameters": {
                "type": "object",
                "properties": {
                    "request": {
                        "type": "string",
                        "description": "The exact booking request from the user."
                    }
                },
                "required": ["request"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("-------------------------- Booking Tool Called ----------------------------");
        Ok(format!(
            "Booking action for '{}' has been simulated.",
            args.request
        ))
    }
}

#[derive(Deserialize, Serialize)]
struct InfoArgs {
    query: String,
}

#[derive(Deserialize, Serialize)]
struct InfoTool;

impl Tool for InfoTool {
    const NAME: &'static str = "info";
    type Error = ToolError;
    type Args = InfoArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "info",
            "description": "Search for general information and answer questions",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The user's question or topic to search for."
                    }
                },
                "required": ["query"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("-------------------------- Info Tool Called ----------------------------");
        Ok(format!(
            "Information request for '{}'. Result: Simulated information retrieval.",
            args.query
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenv();

    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // ── 2. 定义协调员兼分类器 (Coordinator / Classifier Agent) ──────────
    let classifier_agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "Your role is to categorise the user's request using the following values: [booker, info, unclear]\n\
             - booker: For booking flights or hotels.\n\
             - info: For general information questions.\n\
             - unclear: If it fits neither.\n\n\
             Return ONLY the value."
        )
        .temperature(0.0)
        .build();

    // ── 3. 配备工具的默认智能体 (Equipped Default Agent) ───────────────
    // 它能看到所有的可调用工具，但只有在收到对应的明确指令 Prompt 时才去调用特定工具。
    let default_agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "You are a routing agent executing specialized tools. \
             CRITICAL RULE: When a tool returns a result, you MUST output EXACTLY what the tool returned, word for word. \
             Do NOT summarize, do NOT add conversational filler, and do NOT explain anything. Just return the raw tool output.",
        )
        .tool(BookingTool)
        .tool(InfoTool)
        .temperature(0.0)
        .build();

    println!("--- Running Rig Pipeline Routing with Tools ---");

    let scenarios = [
        ("booking request", "Book me a flight to London."),
        ("info request", "What is the capital of Italy?"),
        ("unclear request", "Tell me about quantum physics."),
        ("random info request", "Tell me a random fact."),
        ("future booking", "Find flights to Tokyo next month."),
    ];

    for (name, request) in scenarios.iter() {
        println!("\n--- Running with an {} ---", name);
        println!("Request: '{}'", request);

        // ── 4. 构建并执行动态管道 (Pipeline execution) ─────────────────────────
        // 注意：由于我们在循环中借用 request，每次需根据当前请求重新实例化管道。
        let chain = pipeline::new()
            // a. 提交给分类器进行初步意图判断
            .prompt(classifier_agent.clone())
            // b. 按照语义路由分配不同的执行 Prompt
            .map_ok(|x: String| match x.trim().to_lowercase().as_str() {
                "booker" => Ok(format!(
                    "The user wants to book something. You MUST vividly use the 'booker' tool with this request: '{}'",
                    request
                )),
                "info" => Ok(format!(
                    "The user is asking for info. You MUST vividly use the 'info' tool with this query: '{}'",
                    request
                )),
                "unclear" => Ok(format!(
                    "The user request was unclear. Respond directly to the user asking them to clarify their request: '{}'",
                    request
                )),
                message => Err(format!("Could not process - received category: {message}")),
            })
            // c. 抹平错误嵌套层级 (解析前面的 Result)
            .map(|x| x.unwrap().unwrap())
            // d. 将携带不同重写规则的 Prompt 发送到包含各种工具的终极节点 Agent 进行触发
            .prompt(default_agent.clone());

        match chain.try_call(*request).await {
            Ok(response) => println!("Final Output:\n {}", response.trim()),
            Err(e) => println!("Pipeline Failed: {:?}", e),
        }
    }

    Ok(())
}
