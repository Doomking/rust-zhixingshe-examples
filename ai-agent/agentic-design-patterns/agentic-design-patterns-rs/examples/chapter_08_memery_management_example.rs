//! # Chapter 08: Memory Management (记忆管理模式)
//! 
//! 本示例演示了 **Memory Management** (记忆管理) 设计模式中的 **State (状态管理)**：
//! 将智能体在特定对话期间的动态信息存储在 `Session State` 中，类比人类的短期记忆（工作记忆）。
//! 
//! ## 核心逻辑
//! 1. **Session State (会话状态)**：一个持久化的键值对存储，记录用户偏好、任务状态等。
//! 2. **Tool-based State Update (基于工具的状态更新)**：
//!    - 将状态更新逻辑封装在工具中。
//!    - 智能体通过调用工具来修改自身的“记忆”。
//! 3. **Persistent Storage (持久化存储)**：使用文件系统来持久化会话状态。
//! 
//! 参考 Python 代码，我们实现一个 `log_user_login` 工具来更新登录计数、最后登录时间等状态。

use dotenvy::dotenv;
use rig::client::CompletionClient;
use rig::providers::ollama;
use rig::completion::{ToolDefinition, Prompt};
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;

// ── 0. 定义错误类型 (Error handling) ───────────────────────────────────────

#[derive(Debug)]
struct ToolError(String);

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tool Error: {}", self.0)
    }
}

impl Error for ToolError {}

// ── 1. 定义会话状态 (Session State) ────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Session {
    pub app_name: String,
    pub user_id: String,
    pub session_id: String,
    pub state: HashMap<String, serde_json::Value>,
}

impl Session {
    fn new(app_name: &str, user_id: &str, session_id: &str) -> Self {
        let mut session = Self {
            app_name: app_name.to_string(),
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            state: HashMap::new(),
        };
        
        // 尝试从文件加载状态
        if let Err(e) = session.load_state() {
            println!("加载状态失败: {}", e);
        }
        
        session
    }
    
    fn load_state(&mut self) -> Result<(), Box<dyn Error>> {
        let file_name = format!("./session_{}.json", self.session_id);
        let file_path = Path::new(&file_name);
        if file_path.exists() {
            let content = fs::read_to_string(file_path)?;
            let loaded_state: HashMap<String, serde_json::Value> = serde_json::from_str(&content)?;
            self.state = loaded_state;
        }
        Ok(())
    }
    
    fn save_state(&self) -> Result<(), Box<dyn Error>> {
        let file_name = format!("./session_{}.json", self.session_id);
        let file_path = Path::new(&file_name);
        let content = serde_json::to_string_pretty(&self.state)?;
        fs::write(file_path, content)?;
        Ok(())
    }
}

// ── 2. 定义工具 (Tool) ─────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
struct LoginArgs {
    pub username: String,
}

struct LogUserLoginTool {
    // 模拟 ADK 的 Context，在 Rust 中我们使用 Arc<Mutex<...>> 共享状态
    session: Arc<Mutex<Session>>,
}

impl Tool for LogUserLoginTool {
    const NAME: &'static str = "log_user_login";
    type Error = ToolError;
    type Args = LoginArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(serde_json::json!({
            "name": "log_user_login",
            "description": "在用户登录时更新会话状态。此工具会增加登录计数并记录最后登录时间。",
            "parameters": {
                "type": "object",
                "properties": {
                    "username": {
                        "type": "string",
                        "description": "登录用户的用户名"
                    }
                },
                "required": ["username"]
            }
        }))
        .expect("Failed to parse tool definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut session = self.session.lock().map_err(|_| ToolError("Failed to lock session".to_string()))?;
        
        println!("\n[Tool] 执行 log_user_login，为用户: {}", args.username);

        // 模拟 Python 中的状态更新逻辑
        let login_count = session.state
            .get("user:login_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) + 1;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // 更新状态
        session.state.insert("user:login_count".to_string(), serde_json::json!(login_count));
        session.state.insert("task_status".to_string(), serde_json::json!("active"));
        session.state.insert("user:last_login_ts".to_string(), serde_json::json!(now));
        session.state.insert("temp:validation_needed".to_string(), serde_json::json!(true));
        session.state.insert("user:username".to_string(), serde_json::json!(args.username));

        // 保存状态到持久存储
        if let Err(e) = session.save_state() {
            println!("[Tool] 保存状态失败: {}", e);
        }

        let message = format!("用户 {} 登录已记录。累计登录次数: {}", args.username, login_count);
        println!("[Tool] 状态已更新: {}", message);

        Ok(message)
    }
}

// ── 3. 演示执行 (Main Sequence) ───────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenv();

    // 1. 初始化会话
    let app_name = "state_app_tool";
    let user_id = "user3";
    let session_id = "session3";
    
    let initial_session = Session::new(app_name, user_id, session_id);
    let shared_session = Arc::new(Mutex::new(initial_session));

    println!("Initial state: {:?}", shared_session.lock().unwrap().state);

    // 2. 初始化智能体 (Agent)
    let client: ollama::Client = ollama::Client::new(rig::client::Nothing)?;
    
    // 给智能体配备 LogUserLoginTool
    let agent = client
        .agent("qwen2.5:14b")
        .preamble(
            "你是一个负责用户状态管理的助手。\n\
             当用户登录时，你必须调用 `log_user_login` 工具来更新会话状态。\n\
             请使用中文回答。"
        )
        .tool(LogUserLoginTool {
            session: shared_session.clone(),
        })
        .build();

    // 3. 模拟工具调用逻辑
    println!("\n--- 模拟智能体处理登录请求 ---");
    let query = "用户 'Alice' 刚登录了系统，请记录这个事件。";
    println!("Query: {}", query);

    let response = agent.prompt(query).await?;
    println!("\n--- Agent Final Response ---\n{}", response);

    // 4. 检查更新后的状态
    let final_session = shared_session.lock().unwrap();
    println!("\nState after tool execution: {:#?}", final_session.state);

    Ok(())
}
