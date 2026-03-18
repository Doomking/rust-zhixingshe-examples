//! # Chapter 07: Multi-Agent (多智能体模式)
//!
//! 本示例演示了 **Multi-Agent** (多智能体) 设计模式：
//! 将复杂的业务需求拆分为多个具有专门目标和角色的智能体 (Agent)，分工协作。
//!
//! - **Researcher Agent (研究分析师)**：分析并总结最新技术趋势。
//! - **Writer Agent (技术内容主笔)**：基于初步研究成果撰写引人入胜的博客文章。
//!  
//! 前一个 Agent 的输出将作为上下文输入到下一个 Agent 中，依次顺序执行。

use dotenvy::dotenv;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // ── 0. 环境及客户端初始化 ──────────────────────────────────────────────

    let _ = dotenv();

    // 初始化 Ollama 客户端
    // default url is http://localhost:11434
    let client: ollama::Client = ollama::Client::new(Nothing)?;

    // ── 1. 定义智能体 (Agents) ───────────────────────────────────────────

    // a. 定义具体角色和目标的"研究员"智能体
    let researcher = client
        .agent("qwen2.5:14b")
        .preamble(
            "角色：资深研究分析师\n\
             目标：发现并总结人工智能领域的最新趋势。\n\
             背景：你是一位经验丰富的研究分析师，擅长识别关键趋势并综合信息，能洞察技术发展的脉络。\n\
             请使用中文回答。"
        )
        // .temperature(0.7) // 可选配置
        .build();

    // b. 定义具体角色和目标的"内容编辑"智能体
    let writer = client
        .agent("qwen2.5:14b")
        .preamble(
            "角色：技术内容主笔\n\
             目标：根据研究成果撰写清晰且引人入胜的博客文章。\n\
             背景：你是一位经验丰富的撰稿人，擅长将复杂的技术话题转化为大众易于理解的内容。\n\
             请使用中文回答。"
        )
        // .temperature(0.7) // 可选配置
        .build();

    // ── 2. 定义任务 (Tasks) ──────────────────────────────────────────────

    let research_task = "请调研 2024-2025 年人工智能领域最值得关注的 3 大新兴趋势，重点关注其实际应用场景和潜在影响。\n\
                         预期输出：对这 3 大 AI 趋势的详细总结，包括核心要点和参考来源。";

    let writing_task = "请根据调研成果撰写一篇约 500 字的博客文章，内容应生动有趣，适合普通读者阅读。\n\
                        预期输出：一篇关于最新 AI 趋势、完整约 500 字的中文博客文章。";

    // ── 3. 顺序执行多智能体流程 (Process: Sequential) ────────────────────

    println!("## 正在启动博客创作智能体团队（使用 Ollama qwen2.5:14b）... ##");

    // 第一步：由 researcher 独立完成前期调研任务
    println!("\n[Agent 1: Researcher] 工作中...");
    let research_result = researcher.prompt(research_task).await?;
    println!(
        "--- 调研结果 (Research Findings) ---\n{}\n",
        research_result
    );

    // 第二步：将第一步的调研结果作为上下文(Context)，连同新任务传递给 writer
    println!("\n[Agent 2: Writer] 接收到了调研结果，准备撰写博文...");

    let writer_prompt = format!(
        "任务（写作指令）：\n{}\n\n\
         上下文（来自上一步的调研成果）：\n{}",
        writing_task, research_result
    );

    // 等待作家完成文章
    let final_output = writer.prompt(&writer_prompt).await?;

    // ── 4. 输出最终成果 ──────────────────────────────────────────────────

    println!("\n------------------\n");
    println!("## 智能体团队最终输出 ##");
    println!("{}", final_output);

    Ok(())
}
