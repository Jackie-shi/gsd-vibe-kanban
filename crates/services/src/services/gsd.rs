//! GSD (Get Shit Done) Service
//!
//! This service handles communication with Claude for AI-powered project planning.
//! Supports two modes:
//! 1. Direct API call with ANTHROPIC_API_KEY
//! 2. Claude CLI (claude command) for Claude Code subscribers

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum GsdServiceError {
    #[error("Claude is not configured. Set ANTHROPIC_API_KEY or ensure 'claude' CLI is available.")]
    NotConfigured,
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("API error: {status} - {message}")]
    ApiError { status: u16, message: String },
    #[error("Failed to parse response: {0}")]
    ParseError(String),
    #[error("CLI execution failed: {0}")]
    CliError(String),
}

pub type Result<T> = std::result::Result<T, GsdServiceError>;

// ============================================================================
// Claude API Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
    pub messages: Vec<ClaudeMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResponse {
    pub id: String,
    pub content: Vec<ClaudeContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: ClaudeUsage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeErrorResponse {
    #[serde(rename = "type")]
    pub error_type: String,
    pub error: ClaudeErrorDetail,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// ============================================================================
// GSD Service
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GsdBackend {
    None,
    ApiKey,
    ClaudeCli,
}

#[derive(Clone)]
pub struct GsdService {
    client: Client,
    api_key: Option<String>,
    claude_cli_available: bool,
}

impl Default for GsdService {
    fn default() -> Self {
        Self::new()
    }
}

impl GsdService {
    pub fn new() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        // Check if claude CLI is available
        let claude_cli_available = std::process::Command::new("which")
            .arg("claude")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        Self {
            client: Client::new(),
            api_key,
            claude_cli_available,
        }
    }

    pub fn with_api_key(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key: Some(api_key),
            claude_cli_available: false,
        }
    }

    pub fn backend(&self) -> GsdBackend {
        if self.api_key.is_some() {
            GsdBackend::ApiKey
        } else if self.claude_cli_available {
            GsdBackend::ClaudeCli
        } else {
            GsdBackend::None
        }
    }

    pub fn is_configured(&self) -> bool {
        self.backend() != GsdBackend::None
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend() {
            GsdBackend::ApiKey => "Anthropic API",
            GsdBackend::ClaudeCli => "Claude CLI",
            GsdBackend::None => "Not configured",
        }
    }

    /// Send a message to Claude and get a response
    pub async fn chat(
        &self,
        system_prompt: &str,
        messages: Vec<ClaudeMessage>,
    ) -> Result<String> {
        match self.backend() {
            GsdBackend::ApiKey => self.chat_via_api(system_prompt, messages).await,
            GsdBackend::ClaudeCli => self.chat_via_cli(system_prompt, messages).await,
            GsdBackend::None => Err(GsdServiceError::NotConfigured),
        }
    }

    /// Chat using direct API call
    async fn chat_via_api(
        &self,
        system_prompt: &str,
        messages: Vec<ClaudeMessage>,
    ) -> Result<String> {
        let api_key = self.api_key.as_ref().ok_or(GsdServiceError::NotConfigured)?;

        let request = ClaudeRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            system: Some(system_prompt.to_string()),
            messages,
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();

            // Try to parse as Claude error
            if let Ok(error) = serde_json::from_str::<ClaudeErrorResponse>(&error_body) {
                return Err(GsdServiceError::ApiError {
                    status: status.as_u16(),
                    message: error.error.message,
                });
            }

            return Err(GsdServiceError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let claude_response: ClaudeResponse = response.json().await?;

        // Extract text from response
        let text = claude_response
            .content
            .iter()
            .filter_map(|block| block.text.as_deref())
            .collect::<Vec<&str>>()
            .join("");

        Ok(text)
    }

    /// Chat using Claude CLI
    async fn chat_via_cli(
        &self,
        system_prompt: &str,
        messages: Vec<ClaudeMessage>,
    ) -> Result<String> {
        // Build the conversation history for stdin
        let mut conversation = String::new();
        for msg in &messages {
            let role_label = if msg.role == "user" { "Human" } else { "Assistant" };
            conversation.push_str(&format!("{}: {}\n\n", role_label, msg.content));
        }

        tracing::info!(
            "Calling Claude CLI - system prompt: {} chars, conversation: {} chars",
            system_prompt.len(),
            conversation.len()
        );

        // Spawn the claude process with stdin piped
        // Use --system-prompt for the system instructions
        let mut child = Command::new("claude")
            .arg("-p") // Print mode - non-interactive
            .arg("--no-session-persistence") // Don't save session
            .arg("--system-prompt")
            .arg(system_prompt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| GsdServiceError::CliError(format!("Failed to spawn claude: {}", e)))?;

        // Write the conversation to stdin and close it
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(conversation.as_bytes()).await
                .map_err(|e| GsdServiceError::CliError(format!("Failed to write to stdin: {}", e)))?;
            stdin.shutdown().await
                .map_err(|e| GsdServiceError::CliError(format!("Failed to close stdin: {}", e)))?;
        }

        // Wait for output with timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            child.wait_with_output()
        )
        .await
        .map_err(|_| GsdServiceError::CliError("Claude CLI timed out after 180 seconds".to_string()))?
        .map_err(|e| GsdServiceError::CliError(format!("Failed to wait for claude: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        tracing::info!(
            "Claude CLI completed - status: {}, stdout: {} chars, stderr: {} chars",
            output.status,
            stdout.len(),
            stderr.len()
        );

        if !output.status.success() {
            tracing::error!("Claude CLI failed. stderr: {}, stdout: {}", stderr, stdout);
            return Err(GsdServiceError::CliError(format!(
                "Claude CLI failed: {}",
                if stderr.is_empty() { stdout } else { stderr }
            )));
        }

        Ok(stdout)
    }
}

// ============================================================================
// GSD Prompts
// ============================================================================

/// System prompt for initial project questioning
pub const GSD_QUESTIONING_PROMPT: &str = r#"You are a project planning assistant helping users define and plan their software projects.

Your role is to:
1. Ask clarifying questions to understand the user's vision
2. Help identify the scope and requirements
3. Break down the project into manageable phases
4. Generate actionable tasks

Current stage: Initial questioning

Guidelines:
- Ask ONE question at a time
- Be conversational and helpful
- Focus on understanding the core problem being solved
- Identify target users and their needs
- Understand technical constraints and preferences

When you need user input, respond with a JSON block in this format:
```json
{
  "type": "question",
  "interaction_type": "text|single_choice|multi_choice|confirmation",
  "prompt": "Your question here",
  "options": [
    {"value": "opt1", "label": "Option 1", "description": "Optional description"}
  ]
}
```

When you want to display a message without needing input:
```json
{
  "type": "message",
  "content": "Your message here"
}
```

When you have enough information to generate tasks:
```json
{
  "type": "tasks",
  "phases": [
    {
      "phase_number": 1,
      "phase_name": "Foundation",
      "tasks": [
        {"title": "Task 1", "description": "Description"}
      ]
    }
  ]
}
```

Start by asking the user about their project vision."#;

fn default_interaction_type() -> String {
    "text".to_string()
}

/// Parse GSD response to extract structured data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GsdResponseBlock {
    #[serde(rename = "message")]
    Message { content: String },

    #[serde(rename = "question")]
    Question {
        /// Type of interaction (text, single_choice, multi_choice, confirmation)
        /// Defaults to "text" if not provided
        #[serde(default = "default_interaction_type")]
        interaction_type: String,
        prompt: String,
        #[serde(default)]
        options: Option<Vec<GsdOption>>,
    },

    #[serde(rename = "tasks")]
    Tasks { phases: Vec<GsdPhase> },

    #[serde(rename = "progress")]
    Progress { content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GsdOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GsdPhase {
    pub phase_number: i32,
    pub phase_name: String,
    pub tasks: Vec<GsdTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GsdTask {
    pub title: String,
    pub description: Option<String>,
}

/// Parse Claude's response to extract GSD blocks
pub fn parse_gsd_response(response: &str) -> Vec<GsdResponseBlock> {
    let mut blocks = Vec::new();
    let trimmed = response.trim();

    tracing::debug!("Parsing GSD response: {} chars", trimmed.len());

    // First, try to parse the entire response as JSON (Claude CLI often returns raw JSON)
    if trimmed.starts_with('{') {
        match serde_json::from_str::<GsdResponseBlock>(trimmed) {
            Ok(block) => {
                tracing::debug!("Parsed raw JSON as GsdResponseBlock: {:?}", block);
                blocks.push(block);
                return blocks;
            }
            Err(e) => {
                tracing::debug!("Failed to parse raw JSON: {}. Response: {}", e, &trimmed[..trimmed.len().min(200)]);
            }
        }
    }

    // Find JSON blocks in markdown code blocks
    let mut remaining = response;
    while let Some(start) = remaining.find("```json") {
        let json_start = start + 7;
        if let Some(end) = remaining[json_start..].find("```") {
            let json_str = remaining[json_start..json_start + end].trim();

            match serde_json::from_str::<GsdResponseBlock>(json_str) {
                Ok(block) => {
                    tracing::debug!("Parsed markdown JSON block: {:?}", block);
                    blocks.push(block);
                }
                Err(e) => {
                    tracing::debug!("Failed to parse markdown JSON: {}. JSON: {}", e, &json_str[..json_str.len().min(200)]);
                }
            }

            remaining = &remaining[json_start + end + 3..];
        } else {
            break;
        }
    }

    // If no JSON blocks found, treat the whole response as a message
    if blocks.is_empty() && !trimmed.is_empty() {
        tracing::debug!("No JSON blocks found, treating as plain message");
        blocks.push(GsdResponseBlock::Message {
            content: response.to_string(),
        });
    }

    tracing::debug!("Parsed {} GSD blocks", blocks.len());
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gsd_response_message() {
        let response = r#"```json
{
  "type": "message",
  "content": "Hello!"
}
```"#;

        let blocks = parse_gsd_response(response);
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            GsdResponseBlock::Message { content } => {
                assert_eq!(content, "Hello!");
            }
            _ => panic!("Expected message block"),
        }
    }

    #[test]
    fn test_parse_gsd_response_question() {
        let response = r#"```json
{
  "type": "question",
  "interaction_type": "single_choice",
  "prompt": "What's your goal?",
  "options": [
    {"value": "mvp", "label": "MVP", "description": "Minimal viable product"}
  ]
}
```"#;

        let blocks = parse_gsd_response(response);
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            GsdResponseBlock::Question { interaction_type, prompt, options } => {
                assert_eq!(interaction_type, "single_choice");
                assert_eq!(prompt, "What's your goal?");
                assert!(options.is_some());
            }
            _ => panic!("Expected question block"),
        }
    }

    #[test]
    fn test_parse_gsd_response_question_without_interaction_type() {
        // Test that a question without interaction_type defaults to "text"
        let response = r#"{"type": "question", "prompt": "What would you like to build?"}"#;

        let blocks = parse_gsd_response(response);
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            GsdResponseBlock::Question { interaction_type, prompt, options } => {
                assert_eq!(interaction_type, "text"); // Should default to "text"
                assert_eq!(prompt, "What would you like to build?");
                assert!(options.is_none());
            }
            _ => panic!("Expected question block"),
        }
    }

    #[test]
    fn test_parse_gsd_response_raw_json() {
        // Test parsing raw JSON without markdown wrapper
        let response = r#"{"type": "message", "content": "Hello from raw JSON!"}"#;

        let blocks = parse_gsd_response(response);
        assert_eq!(blocks.len(), 1);

        match &blocks[0] {
            GsdResponseBlock::Message { content } => {
                assert_eq!(content, "Hello from raw JSON!");
            }
            _ => panic!("Expected message block"),
        }
    }
}
