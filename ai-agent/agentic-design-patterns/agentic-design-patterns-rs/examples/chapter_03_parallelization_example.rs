use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
use rig::pipeline::agent_ops::prompt;
use rig::providers::gemini;
use rig::{
    parallel,
    pipeline::{self, Op, passthrough},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 加载 .env 环境变量
    dotenvy::dotenv().ok();

    // 解析命令行参数 (cargo run --example chapter_03_parallelization_example -- [parallel|serial])
    let args: Vec<String> = std::env::args().collect();
    let default_mode = "parallel".to_string();
    let mode = args.get(1).unwrap_or(&default_mode).to_lowercase();

    if mode != "parallel" && mode != "serial" {
        println!("用法: cargo run --example chapter_03_parallelization_example -- [parallel|serial]");
        println!("默认模式: parallel");
        return Ok(());
    }

    // 初始化 Gemini 客户端
    let gemini_client = gemini::Client::from_env();

    let topic = "The history of space exploration";

    let model_name = "gemini-3.1-flash-lite-preview";

    if mode == "parallel" {
        // --- 运行并行版本 ---
        let summarize_agent = gemini_client
            .agent(model_name)
            .preamble("Summarize the following topic concisely:")
            .build();

        let questions_agent = gemini_client
            .agent(model_name)
            .preamble("Generate three interesting questions about the following topic:")
            .build();

        let terms_agent = gemini_client
            .agent(model_name)
            .preamble("Identify 5-10 key terms from the following topic, separated by commas:")
            .build();

        let synthesis_agent = gemini_client
            .agent(model_name)
            .preamble("Synthesize a comprehensive answer based on the provided information.")
            .build();

        let chain = pipeline::new()
            .chain(parallel!(
                prompt(summarize_agent),
                prompt(questions_agent),
                prompt(terms_agent),
                passthrough()
            ))
            .map(|(summary, questions, terms, topic): (Result<String, rig::completion::PromptError>, Result<String, rig::completion::PromptError>, Result<String, rig::completion::PromptError>, String)| {
                let sum_str = summary.unwrap();
                let q_str = questions.unwrap();
                let t_str = terms.unwrap();

                println!("--- [阶段 1] 并行任务完成 ---");
                println!(">> 摘要 (Summary):");
                println!("{}\n", sum_str);
                println!(">> 问题 (Questions):");
                println!("{}\n", q_str);
                println!(">> 关键词 (Key Terms):");
                println!("{}\n", t_str);
                println!("--- [阶段 2] 开始综合生成最终回复 ---\n");

                format!(
                    "Based on the following information:\n\
                     Summary: {}\n\
                     Related Questions: {}\n\
                     Key Terms: {}\n\
                     Synthesize a comprehensive answer for the Original topic: {}",
                    sum_str, q_str, t_str, topic
                )
            })
            .chain(prompt(synthesis_agent));

        println!("\n=======================================================");
        println!("--- 开始并行执行 Rig 示例任务 ---");
        println!("主题: '{}'", topic);
        println!("=======================================================\n");

        let start_parallel = std::time::Instant::now();
        let response_parallel = chain.call(topic.to_string()).await?;
        let duration_parallel = start_parallel.elapsed();

        println!("\n=======================================================");
        println!("--- 并行最终回复 (Parallel Final Response) ---");
        println!("{}", response_parallel);
        println!("--- 并行执行耗时: {:.2?} ---", duration_parallel);
        println!("=======================================================\n");

    } else if mode == "serial" {
        // --- 运行串行版本 ---
        println!("\n=======================================================");
        println!("--- 开始串行执行 Rig 示例任务 ---");
        println!("主题: '{}'", topic);
        println!("=======================================================\n");

        let start_serial = std::time::Instant::now();

        // 1. 摘要
        let summarize_agent_serial = gemini_client
            .agent(model_name)
            .preamble("Summarize the following topic concisely:")
            .build();
        let sum_str = summarize_agent_serial.prompt(topic).await?;

        // 2. 问题
        let questions_agent_serial = gemini_client
            .agent(model_name)
            .preamble("Generate three interesting questions about the following topic:")
            .build();
        let q_str = questions_agent_serial.prompt(topic).await?;

        // 3. 关键词
        let terms_agent_serial = gemini_client
            .agent(model_name)
            .preamble("Identify 5-10 key terms from the following topic, separated by commas:")
            .build();
        let t_str = terms_agent_serial.prompt(topic).await?;

        println!("--- [串行阶段 1] 顺序任务完成 ---");
        println!(">> 摘要 (Summary):");
        println!("{}\n", sum_str);
        println!(">> 问题 (Questions):");
        println!("{}\n", q_str);
        println!(">> 关键词 (Key Terms):");
        println!("{}\n", t_str);
        println!("--- [串行阶段 2] 开始综合生成最终回复 ---\n");

        let synthesis_agent_serial = gemini_client
            .agent(model_name)
            .preamble("Synthesize a comprehensive answer based on the provided information.")
            .build();

        let synthesis_prompt = format!(
            "Based on the following information:\n\
             Summary: {}\n\
             Related Questions: {}\n\
             Key Terms: {}\n\
             Synthesize a comprehensive answer for the Original topic: {}",
            sum_str, q_str, t_str, topic
        );

        let response_serial = synthesis_agent_serial.prompt(&synthesis_prompt).await?;
        let duration_serial = start_serial.elapsed();

        println!("\n=======================================================");
        println!("--- 串行最终回复 (Serial Final Response) ---");
        println!("{}", response_serial);
        println!("--- 串行执行耗时: {:.2?} ---", duration_serial);
        println!("=======================================================\n");
    }

    Ok(())
}
