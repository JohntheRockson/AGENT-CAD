//! Product agent for the shipping in-app chat (Recipe).
//!
//! Owns the system prompt and the JSON IR the single product agent should
//! emit. The server chat loop calls Gemini with [`SYSTEM_PROMPT`] and must
//! keep the last parsed document when the kernel fails.
//!
//! Tool-calling types below are reserved for a later loop; do not add
//! gadgets or extra tooling here.

mod ir_emit;
mod prompt;

pub use ir_emit::{
    example_m8_bolt_document, example_m8_bolt_json, keep_document_on_kernel_failure,
    program_json_for_chat,
};
pub use prompt::SYSTEM_PROMPT;

use serde::{Deserialize, Serialize};

// ── Tool definitions (what the LLM can call) ─────────────────────────────────

/// A tool call emitted by the LLM inside the agent loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum AgentTool {
    /// Write or replace the entire CAD program.
    WriteProgram {
        program: kernel::ir::CadProgram,
    },
    /// Execute the current program and return geometry + metrics.
    RunModel,
    /// Return volume, bbox, surface_area, is_solid for the last-run model.
    Measure,
    /// Return face/edge topology with semantic tags (largest/top/longest, …)
    /// for fillet, shell, and face-based cut/fuse selection.
    ListTopology,
    /// Export the last-run model in a manufacturing format.
    Export { format: ExportFormat },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Step,
    Stl,
    Gltf,
}

// ── Tool result ───────────────────────────────────────────────────────────────

/// Structured result returned to the LLM after a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub success: bool,
    /// JSON payload (mesh metrics, error message, topology list, …).
    pub data: serde_json::Value,
}

// ── Conversation message ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_call: Option<AgentTool>,
    pub tool_result: Option<ToolResult>,
}

// ── Agent configuration (Phase 2) ────────────────────────────────────────────

/// Configuration for the repair loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum number of generate→execute→validate iterations.
    pub max_retries: u8,
    /// LLM model identifier (e.g. "gpt-4o", "claude-opus-4").
    pub model: String,
    /// OpenAI-compatible API base URL.
    pub api_base: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            max_retries: 5,
            model: "gpt-4o".to_string(),
            api_base: "https://api.openai.com/v1".to_string(),
        }
    }
}
