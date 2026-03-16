//! # Chapter 04: Reflection Pattern（反思模式）
//!
//! 本示例演示了 **Reflection Pattern** 设计模式：
//! 通过一个“生成-反思-改进”的循环，由一个 Agent 生成内容，
//! 另一个（或同一个）Agent 对其进行审查并提出建议，再由生成 Agent 根据反馈进行迭代。
//!
//! ## 流程
//! 1. **生成阶段** — 生成初始代码。
//! 2. **反思阶段** — 扮演高级工程师，对代码进行严格审查，寻找漏洞或改进点。
//! 3. **迭代阶段** — 根据反思意见改进代码。
//! 4. **停止条件** — 达到最大迭代次数，或反思者认为代码已达到“完美”。

use dotenvy::dotenv;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 从 .env 文件加载环境变量
    let _ = dotenv();

    // 初始化 Ollama 客户端
    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // --- 1. 核心任务的提示词 ---
    let task_prompt = "Your task is to create a Python function named `calculate_factorial`. \
    This function should do the following: \
    1. Accept a single integer `n` as input. \
    2. Calculate its factorial (n!). \
    3. Include a clear docstring explaining what the function does. \
    4. Handle edge cases: The factorial of 0 is 1. \
    5. Handle invalid input: Raise a ValueError if the input is a negative number.";

    // --- 2. 初始化 Agent ---
    // 生成/改进 Agent
    let generator_agent = client
        .agent("qwen2.5:14b")
        .preamble("You are a helpful assistant that writes clean, efficient Python code.")
        .temperature(0.1)
        .build();

    // 反思/审查 Agent
    let reflector_agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "You are a senior software engineer and an expert in Python. \
             Your role is to perform a meticulous code review. \
             Critically evaluate the provided Python code based on the original task requirements. \
             Look for bugs, style issues, missing edge cases, and areas for improvement. \
             If the code is perfect and meets all requirements, respond with the single phrase 'CODE_IS_PERFECT'. \
             Otherwise, provide a bulleted list of your critiques."
        )
        .temperature(0.1)
        .build();

    // --- 3. 反思循环 ---
    let max_iterations = 3;
    let mut current_code = String::new();

    // 维护简单的对话历史字符串以支持多轮迭代
    let mut history_context = format!("Task: {}\n", task_prompt);

    for i in 0..max_iterations {
        println!(
            "\n{} REFLECTION LOOP: ITERATION {} {}",
            "=".repeat(25),
            i + 1,
            "=".repeat(25)
        );

        // --- STAGE 1: 生成/改进阶段 ---
        let generation_prompt = if i == 0 {
            println!("\n>>> STAGE 1: GENERATING initial code...");
            task_prompt.to_string()
        } else {
            println!("\n>>> STAGE 1: REFINING code based on previous critique...");
            format!(
                "{}\n\nPlease refine the code using the critiques provided.",
                history_context
            )
        };

        let response = generator_agent.prompt(&generation_prompt).await?;
        current_code = response.clone();

        println!("\n--- Generated Code (v{}) ---\n{}", i + 1, current_code);

        // --- STAGE 2: 反思阶段 ---
        println!("\n>>> STAGE 2: REFLECTING on the generated code...");

        let review_prompt = format!(
            "Original Task:\n{}\n\nCode to Review:\n{}",
            task_prompt, current_code
        );

        let critique = reflector_agent.prompt(&review_prompt).await?;

        // --- STAGE 3: 停止条件 ---
        if critique.contains("CODE_IS_PERFECT") {
            println!("\n--- Critique ---\nNo further critiques found. The code is satisfactory.");
            break;
        }

        println!("\n--- Critique ---\n{}", critique);

        // 更新历史记录背景
        history_context.push_str(&format!(
            "\nIteration {} Code:\n{}\nCritique:\n{}\n",
            i + 1,
            current_code,
            critique
        ));
    }

    println!("\n{} FINAL RESULT {}", "=".repeat(30), "=".repeat(30));
    println!("\nFinal refined code after the reflection process:\n");
    println!("{}", current_code);

    Ok(())
}
