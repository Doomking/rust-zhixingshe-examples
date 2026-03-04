use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Panel {
    pub role: String,
    pub dialogue: String,
    pub prompt: String, // Constructed from role + context
}
