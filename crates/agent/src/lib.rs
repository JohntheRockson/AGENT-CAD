//! AgentCAD Agent crate — Phase 2 scaffold.
//!
//! This crate will contain the agentic tool-calling loop:
//!
//!   User prompt → LLM generates JSON IR → kernel executes →
//!   on error/geometry failure → feed exact error back → retry →
//!   repeat until the solid is valid or retries exhausted.
//!
//! In Phase 1 the HTTP server handles the LLM stub. This crate exposes
//! the type definitions so Phase 2 can wire them in without changing the
//! public API of `server`.

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
    /// Return the edge/face topology of the last-run model (for fillet
    /// index selection).
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
