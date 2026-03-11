use rig::pipeline::agent_ops::prompt;
use rig::providers::ollama;
use rig::client::{CompletionClient, Nothing};
use rig::{
    parallel,
    pipeline::{self, Op, passthrough},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ollama_client: ollama::Client = ollama::Client::new(Nothing).expect("Failed to create Ollama client");

    let summarize_agent = ollama_client
        .agent("qwen2.5:14b")
        .preamble("Summarize the following topic concisely:")
        .build();

    let questions_agent = ollama_client
        .agent("qwen2.5:14b")
        .preamble("Generate three interesting questions about the following topic:")
        .build();

    let terms_agent = ollama_client
        .agent("qwen2.5:14b")
        .preamble("Identify 5-10 key terms from the following topic, separated by commas:")
        .build();
        
    let synthesis_agent = ollama_client
        .agent("qwen2.5:14b")
        .preamble("Synthesize a comprehensive answer based on the provided information.")
        .build();

    let chain = pipeline::new()
        .chain(parallel!(
            prompt(summarize_agent),
            prompt(questions_agent),
            prompt(terms_agent),
            passthrough()
        ))
        .map(|(summary, questions, terms, topic)| {
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
                sum_str,
                q_str,
                t_str,
                topic
            )
        })
        .chain(prompt(synthesis_agent));

    let topic = "The history of space exploration";
    println!("\n=======================================================");
    println!("--- 开始并行执行 Rig 示例任务 ---");
    println!("主题: '{}'", topic);
    println!("=======================================================\n");

    let response = chain.call(topic).await?;
    println!("\n=======================================================");
    println!("--- 最终回复 (Final Response) ---");
    println!("{}", response);

    Ok(())
}
