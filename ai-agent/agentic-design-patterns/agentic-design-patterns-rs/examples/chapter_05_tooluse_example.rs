//! # Chapter 05: Tool Use Pattern (工具使用模式)
//!
//! 本示例演示了如何通过给 Agent 配备多个工具，让其自主判断并调用工具来回答问题。
//!
//! ## 核心流程
//! 1. **定义工具集 (Tools)** — 定义三个独立的工具：`WeatherTool` (调用真实 API 获取天气)，`CapitalTool` (查询首都)，以及 `GeneralInfoTool` (查询一般信息)。
//! 2. **配备工具的智能体 (Equipped Agent)** — 创建一个大语言模型 Agent，注册上述工具。
//! 3. **执行查询** — Agent 在接收到查询时，自动调用最合适的工具，并综合工具的返回结果生成最终答案。

use dotenvy::dotenv;
use reqwest;
use rig::client::{CompletionClient, Nothing};
use rig::completion::{Prompt, ToolDefinition};
use rig::providers::ollama;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::error::Error;

// ── 1. 定义工具 (Tools) ──────────────────────────────────────────────

#[derive(Debug)]
struct ToolError(String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolError: {}", self.0)
    }
}

impl std::error::Error for ToolError {}

// --- Weather Tool: 调用真实的 API ---
#[derive(Deserialize, Serialize)]
struct WeatherArgs {
    location: String,
}

#[derive(Deserialize, Serialize)]
struct WeatherTool;

impl Tool for WeatherTool {
    const NAME: &'static str = "get_weather";
    type Error = ToolError;
    type Args = WeatherArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "get_weather",
            "description": "Get the current weather for a specific location using a real API.",
            "parameters": {
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "The city or location to get the weather for, e.g., 'London' or 'Paris'."
                    }
                },
                "required": ["location"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!(
            "\n--- 🛠️ Tool Called: get_weather for location: '{}' ---",
            args.location
        );

        // 1. Get coordinates using geocoding API
        let geo_url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
            args.location
        );
        let geo_res = reqwest::get(&geo_url)
            .await
            .map_err(|_| ToolError("Failed to fetch geo data".to_string()))?;
        let geo_json: serde_json::Value = geo_res
            .json()
            .await
            .map_err(|_| ToolError("Failed to parse geo JSON".to_string()))?;

        if let Some(results) = geo_json.get("results").and_then(|r| r.as_array()) {
            if let Some(first) = results.first() {
                let lat = first
                    .get("latitude")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let lon = first
                    .get("longitude")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let name = first
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&args.location);

                // 2. Get weather
                let req_url = format!(
                    "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current_weather=true",
                    lat, lon
                );
                let weather_res = reqwest::get(&req_url)
                    .await
                    .map_err(|_| ToolError("Failed to fetch weather data".to_string()))?;
                let weather_json: serde_json::Value = weather_res
                    .json()
                    .await
                    .map_err(|_| ToolError("Failed to parse weather JSON".to_string()))?;

                if let Some(current) = weather_json.get("current_weather") {
                    let temp = current
                        .get("temperature")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let wind = current
                        .get("windspeed")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let str_result =
                        format!("Weather in {}: {}°C, wind speed {} km/h", name, temp, wind);
                    println!("--- TOOL RESULT: {} ---", str_result);
                    return Ok(str_result);
                }
            }
        }

        let fallback = format!("Could not find weather for {}", args.location);
        println!("--- TOOL RESULT: {} ---", fallback);
        Ok(fallback)
    }
}

// --- Capital Tool: 模拟的工具 ---
#[derive(Deserialize, Serialize)]
struct CapitalArgs {
    country: String,
}

#[derive(Deserialize, Serialize)]
struct CapitalTool;

impl Tool for CapitalTool {
    const NAME: &'static str = "get_capital";
    type Error = ToolError;
    type Args = CapitalArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "get_capital",
            "description": "Get the capital city of a specific country.",
            "parameters": {
                "type": "object",
                "properties": {
                    "country": {
                        "type": "string",
                        "description": "The country to find the capital for."
                    }
                },
                "required": ["country"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!(
            "\n--- 🛠️ Tool Called: get_capital for country: '{}' ---",
            args.country
        );

        let result = match args.country.to_lowercase().as_str() {
            "france" => "The capital of France is Paris.",
            "japan" => "The capital of Japan is Tokyo.",
            "china" => "The capital of China is Beijing.",
            _ => "I do not have the capital information for that country in my current database.",
        };

        println!("--- TOOL RESULT: {} ---", result);
        Ok(result.to_string())
    }
}

// --- General Info Tool: 模拟的工具 ---
#[derive(Deserialize, Serialize)]
struct GeneralInfoArgs {
    topic: String,
}

#[derive(Deserialize, Serialize)]
struct GeneralInfoTool;

impl Tool for GeneralInfoTool {
    const NAME: &'static str = "get_general_info";
    type Error = ToolError;
    type Args = GeneralInfoArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "get_general_info",
            "description": "Search for general factual information on a topic (like populations, highest mountains, etc).",
            "parameters": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "The topic to retrieve information about."
                    }
                },
                "required": ["topic"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!(
            "\n--- 🛠️ Tool Called: get_general_info for topic: '{}' ---",
            args.topic
        );

        let result = match args.topic.to_lowercase().as_str() {
            "population of earth" => {
                "The estimated population of Earth is around 8 billion people."
            }
            "tallest mountain" => "Mount Everest is the tallest mountain above sea level.",
            _ => "No specific information found, but the topic seems interesting.",
        };

        println!("--- TOOL RESULT: {} ---", result);
        Ok(result.to_string())
    }
}

// ── 2. 主执行入口 ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenv();

    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // Create the agent equipped with all three tools
    let agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "You are a helpful assistant capable of using multiple tools to answer questions. \
                   When a tool provides a result, use it to answer the user's question accurately. \
                   If a tool result says that information is unavailable, inform the user.",
        )
        .tool(WeatherTool)
        .tool(CapitalTool)
        .tool(GeneralInfoTool)
        .temperature(0.0)
        .build();

    let queries = vec![
        "What is the capital of France?",
        "What's the weather like in London?",
        "Tell me something about dogs.",
    ];

    for query in queries {
        println!("\n--- 🏃 Running Agent with Query: '{}' ---", query);

        match agent.prompt(query).await {
            Ok(response) => {
                println!("\n--- ✅ Final Agent Response ---");
                println!("{}", response.trim());
            }
            Err(e) => {
                println!("\n🛑 An error occurred during agent execution: {:?}", e);
            }
        }
    }

    Ok(())
}
