use std::path::PathBuf;

use chrono::Utc;
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    middleware::from_fn_with_state,
    response::{Json as ResponseJson, IntoResponse},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use db::models::gsd_session::{
    CreateGsdSession, CreateGsdMessage, CreateGsdPendingInteraction, CreateGsdGeneratedTask,
    GsdMessage, GsdPendingInteraction, GsdSession, GsdSessionState,
    GsdUserInput, ResolveGsdInteraction, UpdateGsdSession,
    GsdMessageRole, GsdMessageType, GsdGeneratedTask, GsdInteractionType, GsdInteractionOption,
};
use db::models::project::{CreateProject, Project};
use db::models::project_repo::CreateProjectRepo;
use db::models::task::{CreateTask, Task};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use services::services::gsd::{
    GsdService, GsdResponseBlock, ClaudeMessage, parse_gsd_response, GSD_QUESTIONING_PROMPT,
    ResearchFinding, FunctionalRequirement, NonFunctionalRequirement, RoadmapMilestone,
    GsdPhase, GsdTask,
};
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

// ============================================================================
// Path Parameter Structs
// ============================================================================

/// Path parameters for session routes - extracts session_id from any nested path
#[derive(Debug, Deserialize)]
pub struct SessionPathParams {
    pub session_id: Uuid,
    // Other path params are ignored by serde when deserializing
    #[serde(default)]
    pub interaction_id: Option<Uuid>,
}

// ============================================================================
// Middleware
// ============================================================================

async fn load_gsd_session_middleware(
    State(deployment): State<DeploymentImpl>,
    Path(params): Path<SessionPathParams>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    let session_id = params.session_id;
    let session = match GsdSession::find_by_id(&deployment.db().pool, session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            tracing::warn!("GSD session {} not found", session_id);
            return Err(axum::http::StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Failed to fetch GSD session {}: {}", session_id, e);
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut request = request;
    request.extensions_mut().insert(session);
    Ok(next.run(request).await)
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize, TS)]
pub struct GsdSessionsQuery {
    #[serde(default)]
    pub active_only: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SendMessageResponse {
    pub user_message: GsdMessage,
    pub assistant_response: Option<GsdAssistantResponse>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GsdAssistantResponse {
    pub messages: Vec<GsdMessage>,
    pub pending_interaction: Option<GsdPendingInteraction>,
    pub generated_tasks: Vec<GsdGeneratedTask>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct FinalizeSessionRequest {
    pub project_name: String,
    pub repositories: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct FinalizeSessionResponse {
    pub project_id: Uuid,
    pub tasks_created: i32,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Build conversation history for Claude API
fn build_conversation_history(messages: &[GsdMessage]) -> Vec<ClaudeMessage> {
    messages
        .iter()
        .map(|msg| ClaudeMessage {
            role: match msg.role {
                GsdMessageRole::User => "user".to_string(),
                GsdMessageRole::Assistant => "assistant".to_string(),
                GsdMessageRole::System => "user".to_string(), // System messages sent as user
            },
            content: msg.content.clone(),
        })
        .collect()
}

/// Extract project_path from session context JSON
fn get_project_path_from_session(session: &GsdSession) -> Option<PathBuf> {
    serde_json::from_str::<serde_json::Value>(&session.context)
        .ok()
        .and_then(|v| v.get("project_path")?.as_str().map(PathBuf::from))
}

/// Write a file to the .planning/ directory, creating it if needed
async fn write_planning_file(project_path: &std::path::Path, filename: &str, content: &str) {
    let planning_dir = project_path.join(".planning");
    if let Err(e) = tokio::fs::create_dir_all(&planning_dir).await {
        tracing::error!("Failed to create .planning/ directory at {:?}: {}", planning_dir, e);
        return;
    }

    let file_path = planning_dir.join(filename);
    match tokio::fs::write(&file_path, content).await {
        Ok(_) => tracing::info!("Written .planning/{} at {:?}", filename, file_path),
        Err(e) => tracing::error!("Failed to write .planning/{}: {}", filename, e),
    }
}

/// Generate PROJECT.md content from session info
fn generate_project_md(title: &str, context: &str) -> String {
    let mut output = format!("# {}\n\n", title);
    output.push_str("## Project Overview\n\n");

    // Try to extract project_path from context
    if let Ok(ctx) = serde_json::from_str::<serde_json::Value>(context) {
        if let Some(path) = ctx.get("project_path").and_then(|v| v.as_str()) {
            output.push_str(&format!("**Project Path:** `{}`\n\n", path));
        }
    }

    output.push_str("*Generated by GSD (Get Shit Done) workflow*\n");
    output
}

/// Generate STATE.md content
fn generate_state_md() -> String {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    format!(
        "# Project State\n\n\
        ## Current Status\n\n\
        - **Phase:** Planning\n\
        - **Last Updated:** {}\n\n\
        ## Progress\n\n\
        - [x] Project questioning completed\n\
        - [x] Research summary generated\n\
        - [ ] Requirements approved\n\
        - [ ] Roadmap approved\n\
        - [ ] Phase 1 tasks generated\n",
        now
    )
}

/// Generate config.json content
fn generate_config_json(title: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "project_name": title,
        "version": "1.0.0",
        "created_at": Utc::now().to_rfc3339(),
        "workflow": {
            "current_phase": "planning",
            "phases_completed": []
        }
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Generate a slug from a string (lowercase, hyphenated)
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Generate PLAN.md content for a phase
fn generate_phase_plan(phase_number: i32, phase_name: &str, tasks: &[GsdTask]) -> String {
    let now = Utc::now().format("%Y-%m-%d");
    let mut output = format!(
        "# Phase {}: {}\n\n\
        > Generated: {}\n\n\
        ## Overview\n\n\
        This phase contains {} task(s).\n\n\
        ## Tasks\n\n",
        phase_number, phase_name, now, tasks.len()
    );

    for (idx, task) in tasks.iter().enumerate() {
        let task_num = idx + 1;
        output.push_str(&format!("### {}. {}\n\n", task_num, task.title));

        if let Some(ref desc) = task.description {
            // Only show first line as summary in PLAN.md
            let summary = desc.lines().next().unwrap_or("");
            output.push_str(&format!("{}\n\n", summary));
        }

        if let Some(ref criteria) = task.success_criteria {
            if !criteria.is_empty() {
                output.push_str("**Success Criteria:**\n");
                for c in criteria {
                    output.push_str(&format!("- [ ] {}\n", c));
                }
                output.push('\n');
            }
        }
    }

    output.push_str("## Progress\n\n");
    output.push_str("| Task | Status | Notes |\n");
    output.push_str("|------|--------|-------|\n");
    for (idx, task) in tasks.iter().enumerate() {
        output.push_str(&format!("| {}. {} | ⏳ Pending | |\n", idx + 1, task.title));
    }

    output
}

/// Generate markdown content for a single task file
fn generate_task_file(
    task_order: i32,
    task: &GsdTask,
    phase_number: i32,
    phase_name: &str,
) -> String {
    let now = Utc::now().format("%Y-%m-%d");
    let mut output = format!(
        "# Task {}.{}: {}\n\n\
        > Phase: {} - {}\n\
        > Generated: {}\n\
        > Status: ⏳ Pending\n\n",
        phase_number, task_order, task.title,
        phase_number, phase_name, now
    );

    // Add description (which should now contain Overview, Implementation Steps, Technical Notes)
    if let Some(ref desc) = task.description {
        output.push_str(desc);
        output.push_str("\n\n");
    }

    // Add requirements references
    if let Some(ref reqs) = task.requirements {
        if !reqs.is_empty() {
            output.push_str("## Related Requirements\n\n");
            for req in reqs {
                output.push_str(&format!("- {}\n", req));
            }
            output.push('\n');
        }
    }

    // Add success criteria as checklist
    if let Some(ref criteria) = task.success_criteria {
        if !criteria.is_empty() {
            output.push_str("## Acceptance Criteria\n\n");
            for c in criteria {
                output.push_str(&format!("- [ ] {}\n", c));
            }
            output.push('\n');
        }
    }

    // Add execution log section
    output.push_str("---\n\n## Execution Log\n\n");
    output.push_str("*Record your progress, decisions, and notes here as you work on this task.*\n\n");
    output.push_str("### Session Notes\n\n");
    output.push_str("<!-- Add your notes below -->\n\n");

    output
}

/// Write phase files to .planning/phases/ directory
async fn write_phase_files(
    project_path: &std::path::Path,
    phases: &[GsdPhase],
) {
    let phases_dir = project_path.join(".planning").join("phases");

    for phase in phases {
        // Create phase directory: phase-1-foundation
        let phase_slug = slugify(&phase.phase_name);
        let phase_dir_name = format!("phase-{}-{}", phase.phase_number, phase_slug);
        let phase_dir = phases_dir.join(&phase_dir_name);

        if let Err(e) = tokio::fs::create_dir_all(&phase_dir).await {
            tracing::error!("Failed to create phase directory {:?}: {}", phase_dir, e);
            continue;
        }

        // Write PLAN.md for the phase
        let plan_content = generate_phase_plan(phase.phase_number, &phase.phase_name, &phase.tasks);
        let plan_path = phase_dir.join("PLAN.md");
        if let Err(e) = tokio::fs::write(&plan_path, &plan_content).await {
            tracing::error!("Failed to write {:?}: {}", plan_path, e);
        } else {
            tracing::info!("Written {:?}", plan_path);
        }

        // Write individual task files
        for (idx, task) in phase.tasks.iter().enumerate() {
            let task_order = idx as i32 + 1;
            let task_slug = slugify(&task.title);
            // Truncate slug to reasonable length
            let task_slug_short = task_slug.chars().take(50).collect::<String>();
            let task_filename = format!("task-{:02}-{}.md", task_order, task_slug_short);

            let task_content = generate_task_file(task_order, task, phase.phase_number, &phase.phase_name);
            let task_path = phase_dir.join(&task_filename);

            if let Err(e) = tokio::fs::write(&task_path, &task_content).await {
                tracing::error!("Failed to write {:?}: {}", task_path, e);
            } else {
                tracing::info!("Written {:?}", task_path);
            }
        }
    }

    tracing::info!("Written phase files for {} phases to {:?}", phases.len(), phases_dir);
}

/// Process Claude's response into messages, interactions, and tasks
/// IMPORTANT: Only processes up to the first Question or Tasks block to ensure
/// step-by-step interaction with the user
async fn process_claude_response(
    pool: &sqlx::SqlitePool,
    session_id: Uuid,
    claude_response: &str,
    project_path: Option<&std::path::Path>,
    session_title: &str,
    session_context: &str,
) -> Result<GsdAssistantResponse, ApiError> {
    tracing::debug!("Processing Claude response for session {}", session_id);
    let blocks = parse_gsd_response(claude_response);
    tracing::debug!("Parsed {} blocks from Claude response", blocks.len());

    let mut messages = Vec::new();
    let mut pending_interaction = None;
    let mut generated_tasks = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        tracing::debug!("Processing block {}: {:?}", i, block);
        match block {
            GsdResponseBlock::Message { content, stage } => {
                let metadata = stage.map(|s| serde_json::json!({ "stage": s }).to_string());
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content,
                    message_type: GsdMessageType::Message,
                    metadata,
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);
            }
            GsdResponseBlock::Progress { content } => {
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content,
                    message_type: GsdMessageType::Progress,
                    metadata: None,
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);
            }
            GsdResponseBlock::Question { interaction_type, prompt, options, stage } => {
                tracing::debug!("Processing Question block - type: {}, has_options: {}, stage: {:?}", interaction_type, options.is_some(), stage);

                // First create a message for the question
                let metadata = stage.as_ref().map(|s| serde_json::json!({ "stage": s }).to_string());
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: prompt.clone(),
                    message_type: GsdMessageType::Question,
                    metadata,
                };
                let msg_id = Uuid::new_v4();
                tracing::debug!("Creating question message with id: {}", msg_id);
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await.map_err(|e| {
                    tracing::error!("Failed to create question message: {}", e);
                    e
                })?;
                messages.push(msg);
                tracing::debug!("Question message created successfully");

                // Then create the pending interaction
                let int_type = match interaction_type.as_str() {
                    "single_choice" => GsdInteractionType::SingleChoice,
                    "multi_choice" => GsdInteractionType::MultiChoice,
                    "confirmation" => GsdInteractionType::Confirmation,
                    _ => GsdInteractionType::Text,
                };

                let int_options: Option<Vec<GsdInteractionOption>> = options.map(|opts| {
                    opts.into_iter()
                        .map(|opt| GsdInteractionOption {
                            value: opt.value,
                            label: opt.label,
                            description: opt.description,
                        })
                        .collect()
                });

                let int_data = CreateGsdPendingInteraction {
                    session_id,
                    interaction_type: int_type,
                    prompt,
                    options: int_options,
                    metadata: stage.map(|s| serde_json::json!({ "stage": s }).to_string()),
                };
                let int_id = Uuid::new_v4();
                tracing::debug!("Creating pending interaction with id: {}", int_id);
                pending_interaction = Some(GsdPendingInteraction::create(pool, &int_data, int_id).await.map_err(|e| {
                    tracing::error!("Failed to create pending interaction: {}", e);
                    e
                })?);
                tracing::debug!("Pending interaction created successfully");

                // STOP processing after the first question - ensure step-by-step interaction
                tracing::debug!("Stopping after first question to ensure step-by-step interaction");
                break;
            }
            GsdResponseBlock::ResearchSummary { findings, recommendations } => {
                // Format research summary as a nice display
                let content = format_research_summary(&findings, &recommendations);
                let metadata = serde_json::json!({
                    "stage": "research",
                    "findings": findings,
                    "recommendations": recommendations
                }).to_string();

                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: content.clone(),
                    message_type: GsdMessageType::ResearchSummary,
                    metadata: Some(metadata),
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);

                // Write .planning/ files on first structured output
                if let Some(path) = project_path {
                    // Create PROJECT.md and config.json on first file write
                    write_planning_file(path, "PROJECT.md", &generate_project_md(session_title, session_context)).await;
                    write_planning_file(path, "config.json", &generate_config_json(session_title)).await;

                    // Create research/ subdirectory and write summary
                    let research_dir = path.join(".planning").join("research");
                    if let Err(e) = tokio::fs::create_dir_all(&research_dir).await {
                        tracing::error!("Failed to create .planning/research/ directory: {}", e);
                    }
                    write_planning_file(path, "research/SUMMARY.md", &content).await;
                    tracing::info!("Written .planning/ research files to {:?}", path);
                }
            }
            GsdResponseBlock::Requirements { functional, non_functional } => {
                // Format requirements as a nice display
                let content = format_requirements(&functional, &non_functional);
                let metadata = serde_json::json!({
                    "stage": "requirements",
                    "functional": functional,
                    "non_functional": non_functional
                }).to_string();

                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: content.clone(),
                    message_type: GsdMessageType::Requirements,
                    metadata: Some(metadata),
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);

                // Write REQUIREMENTS.md
                if let Some(path) = project_path {
                    write_planning_file(path, "REQUIREMENTS.md", &content).await;
                    tracing::info!("Written .planning/REQUIREMENTS.md to {:?}", path);
                }
            }
            GsdResponseBlock::Roadmap { milestones } => {
                // Format roadmap as a nice display
                let content = format_roadmap(&milestones);
                let metadata = serde_json::json!({
                    "stage": "roadmap",
                    "milestones": milestones
                }).to_string();

                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: content.clone(),
                    message_type: GsdMessageType::Roadmap,
                    metadata: Some(metadata),
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);

                // Write ROADMAP.md and STATE.md
                if let Some(path) = project_path {
                    write_planning_file(path, "ROADMAP.md", &content).await;
                    write_planning_file(path, "STATE.md", &generate_state_md()).await;
                    tracing::info!("Written .planning/ROADMAP.md and STATE.md to {:?}", path);
                }
            }
            GsdResponseBlock::Tasks { phases } => {
                // Create message about tasks
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: format!("I've created {} phase(s) with tasks for your project. Please review them below.", phases.len()),
                    message_type: GsdMessageType::TasksPreview,
                    metadata: Some(serde_json::json!({ "stage": "tasks" }).to_string()),
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);

                // Create the tasks in database
                for phase in &phases {
                    for (idx, task) in phase.tasks.iter().enumerate() {
                        let task_data = CreateGsdGeneratedTask {
                            session_id,
                            phase_number: phase.phase_number,
                            phase_name: phase.phase_name.clone(),
                            task_order: idx as i32 + 1,
                            title: task.title.clone(),
                            description: task.description.clone(),
                            requirements: task.requirements.clone(),
                            success_criteria: task.success_criteria.clone(),
                            dependencies: None,
                        };
                        let task_id = Uuid::new_v4();
                        let created_task = GsdGeneratedTask::create(pool, &task_data, task_id).await?;
                        generated_tasks.push(created_task);
                    }
                }

                // Write phase files to .planning/phases/
                if let Some(path) = project_path {
                    write_phase_files(path, &phases).await;
                    tracing::info!("Written phase files to {:?}/.planning/phases/", path);
                }

                // STOP processing after tasks - this is the final output
                tracing::debug!("Stopping after tasks block");
                break;
            }
        }
    }

    tracing::debug!(
        "Finished processing - {} messages, interaction: {}, {} tasks",
        messages.len(),
        pending_interaction.is_some(),
        generated_tasks.len()
    );

    Ok(GsdAssistantResponse {
        messages,
        pending_interaction,
        generated_tasks,
    })
}

/// Format research summary for display
fn format_research_summary(findings: &[ResearchFinding], recommendations: &str) -> String {
    let mut output = String::from("## 📚 Research Summary\n\n");

    for finding in findings {
        output.push_str(&format!("### {}\n", finding.category));
        for item in &finding.items {
            output.push_str(&format!("- {}\n", item));
        }
        output.push('\n');
    }

    output.push_str(&format!("### 💡 Recommendations\n{}\n", recommendations));
    output
}

/// Format requirements for display
fn format_requirements(functional: &[FunctionalRequirement], non_functional: &[NonFunctionalRequirement]) -> String {
    let mut output = String::from("## 📋 Requirements\n\n");

    output.push_str("### Functional Requirements\n\n");
    for req in functional {
        output.push_str(&format!("**{}** - {} ({})\n", req.id, req.title, req.priority));
        output.push_str(&format!("{}\n", req.description));
        if !req.user_stories.is_empty() {
            output.push_str("User Stories:\n");
            for story in &req.user_stories {
                output.push_str(&format!("- {}\n", story));
            }
        }
        output.push('\n');
    }

    output.push_str("### Non-Functional Requirements\n\n");
    for req in non_functional {
        output.push_str(&format!("**{}** [{}]\n", req.id, req.category));
        output.push_str(&format!("{}\n", req.requirement));
        if let Some(ref criteria) = req.acceptance_criteria {
            output.push_str(&format!("Acceptance: {}\n", criteria));
        }
        output.push('\n');
    }

    output
}

/// Format roadmap for display
fn format_roadmap(milestones: &[RoadmapMilestone]) -> String {
    let mut output = String::from("## 🗺️ Project Roadmap\n\n");

    for milestone in milestones {
        output.push_str(&format!("### Phase {} - {}\n", milestone.phase, milestone.name));
        output.push_str(&format!("**Goal:** {}\n\n", milestone.goal));

        if !milestone.success_criteria.is_empty() {
            output.push_str("**Success Criteria:**\n");
            for criteria in &milestone.success_criteria {
                output.push_str(&format!("- {}\n", criteria));
            }
        }

        if let Some(tasks) = milestone.estimated_tasks {
            output.push_str(&format!("\n*Estimated tasks: {}*\n", tasks));
        }
        output.push('\n');
    }

    output
}

/// Create a fallback response when Claude API is not configured
fn create_fallback_response() -> &'static str {
    r#"```json
{
  "type": "message",
  "content": "Welcome! I'm here to help you plan your project. To enable AI-powered planning, please configure your Anthropic API key in Settings.\n\nFor now, you can still create a project manually using the 'Quick Create' option."
}
```"#
}

// ============================================================================
// GSD CLI WebSocket Types
// ============================================================================

/// Query parameters for GSD CLI WebSocket connection
#[derive(Debug, Deserialize)]
pub struct GsdCliQuery {
    /// Path to the project directory where GSD will run
    pub project_path: String,
    /// GSD skill to run (default: new-project)
    #[serde(default = "default_gsd_skill")]
    pub skill: String,
    /// Terminal columns (default: 120)
    #[serde(default = "default_cli_cols")]
    pub cols: u16,
    /// Terminal rows (default: 40)
    #[serde(default = "default_cli_rows")]
    pub rows: u16,
}

fn default_gsd_skill() -> String {
    "new-project".to_string()
}

fn default_cli_cols() -> u16 {
    120
}

fn default_cli_rows() -> u16 {
    40
}

/// Commands that can be sent from the frontend to the GSD CLI
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GsdCliCommand {
    /// Send input text to the CLI (for answering prompts)
    Input { data: String },
    /// Resize the terminal
    Resize { cols: u16, rows: u16 },
    /// Abort the current GSD session
    Abort,
}

/// Messages sent from the backend to the frontend
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GsdCliMessage {
    /// Raw CLI output (base64 encoded for binary safety)
    Output { data: String },
    /// Session started successfully
    Started { session_id: String },
    /// Session completed (process exited)
    Completed { exit_code: Option<i32> },
    /// Error occurred
    Error { message: String },
    /// GSD stage changed (detected from output)
    StageChanged { stage: String },
}

// ============================================================================
// Handlers
// ============================================================================

/// List all GSD sessions
pub async fn list_sessions(
    State(deployment): State<DeploymentImpl>,
    axum::extract::Query(params): axum::extract::Query<GsdSessionsQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<GsdSession>>>, ApiError> {
    let sessions = if params.active_only {
        GsdSession::find_active(&deployment.db().pool).await?
    } else {
        GsdSession::find_all(&deployment.db().pool).await?
    };

    Ok(ResponseJson(ApiResponse::success(sessions)))
}

/// Create a new GSD session
pub async fn create_session(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateGsdSession>,
) -> Result<ResponseJson<ApiResponse<GsdSessionState>>, ApiError> {
    let session_id = Uuid::new_v4();
    let session = GsdSession::create(&deployment.db().pool, &payload, session_id).await?;

    // Create initial welcome message from assistant
    // If project_path is provided, include it in the welcome message
    let welcome_content = if let Some(ref path) = payload.project_path {
        format!(
            "Welcome! I'm here to help you plan your project.\n\n\
            📁 Project Directory: `{}`\n\n\
            Tell me about what you want to build - describe your vision, the problem you're solving, or the product you have in mind.",
            path
        )
    } else {
        "Welcome! I'm here to help you plan your project. Tell me about what you want to build - describe your vision, the problem you're solving, or the product you have in mind.".to_string()
    };

    let welcome_message = CreateGsdMessage {
        session_id: session.id,
        role: GsdMessageRole::Assistant,
        content: welcome_content,
        message_type: GsdMessageType::Message,
        metadata: payload.project_path.as_ref().map(|p| {
            serde_json::json!({ "project_path": p }).to_string()
        }),
    };
    let msg_id = Uuid::new_v4();
    GsdMessage::create(&deployment.db().pool, &welcome_message, msg_id).await?;

    // Store project_path in session context if provided
    if let Some(ref path) = payload.project_path {
        let update = UpdateGsdSession {
            title: None,
            status: None,
            stage: None,
            context: Some(serde_json::json!({ "project_path": path }).to_string()),
            claude_conversation_id: None,
            project_id: None,
        };
        GsdSession::update(&deployment.db().pool, session.id, &update).await?;
    }

    // Get full state
    let state = GsdSession::get_state(&deployment.db().pool, session.id)
        .await?
        .ok_or_else(|| ApiError::Database(sqlx::Error::RowNotFound))?;

    deployment
        .track_if_analytics_allowed(
            "gsd_session_created",
            serde_json::json!({
                "session_id": session.id.to_string(),
                "title": session.title,
                "has_project_path": payload.project_path.is_some(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(state)))
}

/// Get a specific GSD session with full state
pub async fn get_session(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<GsdSessionState>>, ApiError> {
    let state = GsdSession::get_state(&deployment.db().pool, session.id)
        .await?
        .ok_or_else(|| ApiError::Database(sqlx::Error::RowNotFound))?;

    Ok(ResponseJson(ApiResponse::success(state)))
}

/// Update a GSD session
pub async fn update_session(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<UpdateGsdSession>,
) -> Result<ResponseJson<ApiResponse<GsdSession>>, ApiError> {
    let updated = GsdSession::update(&deployment.db().pool, session.id, &payload).await?;

    Ok(ResponseJson(ApiResponse::success(updated)))
}

/// Delete a GSD session
pub async fn delete_session(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let rows = GsdSession::delete(&deployment.db().pool, session.id).await?;
    if rows == 0 {
        Err(ApiError::Database(sqlx::Error::RowNotFound))
    } else {
        Ok(ResponseJson(ApiResponse::success(())))
    }
}

/// Send a message in the GSD session
/// This is the main interaction endpoint - sends user input to Claude and gets response
pub async fn send_message(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<ResponseJson<ApiResponse<SendMessageResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // 1. Store user message
    let user_msg_data = CreateGsdMessage {
        session_id: session.id,
        role: GsdMessageRole::User,
        content: payload.content.clone(),
        message_type: GsdMessageType::Message,
        metadata: None,
    };
    let user_msg_id = Uuid::new_v4();
    let user_message = GsdMessage::create(pool, &user_msg_data, user_msg_id).await?;

    // 2. Get conversation history
    let all_messages = GsdMessage::find_by_session_id(pool, session.id).await?;
    let conversation_history = build_conversation_history(&all_messages);

    // 3. Call Claude API
    let gsd_service = GsdService::new();
    let claude_response = if gsd_service.is_configured() {
        match gsd_service.chat(GSD_QUESTIONING_PROMPT, conversation_history).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!("Claude API error: {}", e);
                // Return a helpful error message
                format!(
                    r#"```json
{{"type": "message", "content": "I encountered an error communicating with the AI service: {}. Please try again."}}
```"#,
                    e
                )
            }
        }
    } else {
        create_fallback_response().to_string()
    };

    // 4. Process Claude's response (with file generation if project_path is set)
    let project_path = get_project_path_from_session(&session);
    let assistant_response = process_claude_response(
        pool,
        session.id,
        &claude_response,
        project_path.as_deref(),
        &session.title,
        &session.context,
    ).await?;

    let response = SendMessageResponse {
        user_message,
        assistant_response: Some(assistant_response),
    };

    Ok(ResponseJson(ApiResponse::success(response)))
}

/// Resolve a pending interaction (user answers a question)
pub async fn resolve_interaction(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
    Path(params): Path<SessionPathParams>,
    Json(payload): Json<ResolveGsdInteraction>,
) -> Result<ResponseJson<ApiResponse<GsdAssistantResponse>>, ApiError> {
    let interaction_id = params.interaction_id.ok_or_else(|| {
        ApiError::BadRequest("Missing interaction_id in path".to_string())
    })?;
    tracing::info!("=== RESOLVE_INTERACTION V4 === session: {}, interaction: {}", session.id, interaction_id);
    tracing::debug!("Payload: {:?}", payload);
    let pool = &deployment.db().pool;

    // 1. Resolve the pending interaction
    let resolved = GsdPendingInteraction::resolve(
        pool,
        interaction_id,
        &payload.response,
    ).await?;

    // 2. Store the user's response as a message
    // Format the response nicely for the conversation
    let response_text = match &payload.response {
        GsdUserInput::Text(text) => text.clone(),
        GsdUserInput::SingleChoice(value) => {
            // Try to get the label from the resolved interaction's options
            if let Some(opts) = &resolved.options {
                if let Ok(options) = serde_json::from_str::<Vec<GsdInteractionOption>>(opts) {
                    options
                        .iter()
                        .find(|o| &o.value == value)
                        .map(|o| o.label.clone())
                        .unwrap_or_else(|| value.clone())
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            }
        }
        GsdUserInput::MultiChoice(values) => values.join(", "),
        GsdUserInput::Confirmation(confirmed) => {
            if *confirmed { "Yes".to_string() } else { "No".to_string() }
        }
    };

    let user_msg_data = CreateGsdMessage {
        session_id: session.id,
        role: GsdMessageRole::User,
        content: response_text,
        message_type: GsdMessageType::Message,
        metadata: Some(serde_json::json!({
            "interaction_id": interaction_id.to_string(),
            "interaction_type": format!("{:?}", resolved.interaction_type),
        }).to_string()),
    };
    let user_msg_id = Uuid::new_v4();
    GsdMessage::create(pool, &user_msg_data, user_msg_id).await?;

    // 3. Get updated conversation history
    let all_messages = GsdMessage::find_by_session_id(pool, session.id).await?;
    let conversation_history = build_conversation_history(&all_messages);

    // 4. Call Claude API to continue the conversation
    let gsd_service = GsdService::new();
    let claude_response = if gsd_service.is_configured() {
        match gsd_service.chat(GSD_QUESTIONING_PROMPT, conversation_history).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!("Claude API error: {}", e);
                format!(
                    r#"```json
{{"type": "message", "content": "I encountered an error: {}. Please try again."}}
```"#,
                    e
                )
            }
        }
    } else {
        // Fallback for unconfigured API
        r#"```json
{"type": "message", "content": "Thank you for your input! To continue with AI-powered planning, please configure your Anthropic API key in Settings."}
```"#.to_string()
    };

    // 5. Process Claude's response (with file generation if project_path is set)
    let project_path = get_project_path_from_session(&session);
    let response = process_claude_response(
        pool,
        session.id,
        &claude_response,
        project_path.as_deref(),
        &session.title,
        &session.context,
    ).await?;

    tracing::debug!("resolve_interaction completed - returning {} messages", response.messages.len());
    Ok(ResponseJson(ApiResponse::success(response)))
}

/// Get messages for a session
pub async fn get_messages(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<GsdMessage>>>, ApiError> {
    let messages = GsdMessage::find_by_session_id(&deployment.db().pool, session.id).await?;
    Ok(ResponseJson(ApiResponse::success(messages)))
}

/// Get generated tasks for a session
pub async fn get_generated_tasks(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<GsdGeneratedTask>>>, ApiError> {
    let tasks = GsdGeneratedTask::find_by_session_id(&deployment.db().pool, session.id).await?;
    Ok(ResponseJson(ApiResponse::success(tasks)))
}

/// Approve all generated tasks
pub async fn approve_all_tasks(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<i64>>, ApiError> {
    let count = GsdGeneratedTask::approve_all_by_session(&deployment.db().pool, session.id).await?;
    Ok(ResponseJson(ApiResponse::success(count as i64)))
}

/// Finalize session and create project with tasks
/// If the session already has a project_id (linked to existing project), tasks will be created
/// in that project. Otherwise, a new project will be created.
pub async fn finalize_session(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<FinalizeSessionRequest>,
) -> Result<ResponseJson<ApiResponse<FinalizeSessionResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // 1. Get all approved tasks
    let generated_tasks = GsdGeneratedTask::find_by_session_id(pool, session.id).await?;
    tracing::info!(
        "Finalize session {}: found {} generated tasks total",
        session.id,
        generated_tasks.len()
    );
    let approved_tasks: Vec<_> = generated_tasks.into_iter().filter(|t| t.approved).collect();
    tracing::info!(
        "Finalize session {}: {} approved tasks",
        session.id,
        approved_tasks.len()
    );

    if approved_tasks.is_empty() {
        tracing::warn!(
            "Finalize session {}: no approved tasks found! Was approve_all_tasks called?",
            session.id
        );
        return Err(ApiError::BadRequest("No approved tasks to create project from".to_string()));
    }

    // 2. Get or create the project
    let final_project_id = if let Some(existing_project_id) = session.project_id {
        // Session is linked to an existing project - use it
        tracing::info!(
            "Session {} already linked to project {}, adding tasks to existing project",
            session.id,
            existing_project_id
        );
        existing_project_id
    } else {
        // No existing project - create a new one
        let project_id = Uuid::new_v4();
        let create_project_data = CreateProject {
            name: payload.project_name.clone(),
            repositories: payload.repositories.iter().map(|path| CreateProjectRepo {
                display_name: path.split('/').last().unwrap_or(path).to_string(),
                git_repo_path: path.clone(),
            }).collect(),
        };

        let project = Project::create(pool, &create_project_data, project_id).await
            .map_err(|e| ApiError::Database(e))?;

        tracing::info!(
            "Created new project {} for session {}",
            project.id,
            session.id
        );
        project.id
    };

    // 3. Create tasks from approved generated tasks (preserving phase info)
    tracing::info!(
        "Creating {} tasks for project {} from session {}",
        approved_tasks.len(),
        final_project_id,
        session.id
    );
    let mut tasks_created = 0;
    for gsd_task in &approved_tasks {
        let task_id = Uuid::new_v4();
        let create_task_data = CreateTask::from_gsd_task(
            final_project_id,
            gsd_task.title.clone(),
            gsd_task.description.clone(),
            gsd_task.phase_number,
            gsd_task.phase_name.clone(),
            gsd_task.task_order,
        );

        match Task::create(pool, &create_task_data, task_id).await {
            Ok(_) => {
                tasks_created += 1;
                tracing::debug!(
                    "Created task {} ({}) - phase {}, order {}",
                    task_id,
                    gsd_task.title,
                    gsd_task.phase_number,
                    gsd_task.task_order
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create task '{}' (phase {}, order {}): {}",
                    gsd_task.title,
                    gsd_task.phase_number,
                    gsd_task.task_order,
                    e
                );
                return Err(ApiError::Database(e));
            }
        }
    }

    // 4. Update the GSD session with the project_id and mark as completed
    let update_data = UpdateGsdSession {
        title: None,
        status: Some(db::models::gsd_session::GsdSessionStatus::Completed),
        stage: Some("completed".to_string()),
        context: None,
        claude_conversation_id: None,
        project_id: Some(final_project_id),
    };
    GsdSession::update(pool, session.id, &update_data).await?;

    let response = FinalizeSessionResponse {
        project_id: final_project_id,
        tasks_created,
    };

    deployment
        .track_if_analytics_allowed(
            "gsd_session_finalized",
            serde_json::json!({
                "session_id": session.id.to_string(),
                "project_id": final_project_id.to_string(),
                "tasks_created": tasks_created,
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(response)))
}

// ============================================================================
// Status
// ============================================================================

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GsdStatusResponse {
    pub api_configured: bool,
    pub backend: String,
    pub message: String,
}

/// Check if GSD (Claude API or CLI) is properly configured
pub async fn get_status() -> Result<ResponseJson<ApiResponse<GsdStatusResponse>>, ApiError> {
    let gsd_service = GsdService::new();
    let configured = gsd_service.is_configured();
    let backend_name = gsd_service.backend_name();

    let response = GsdStatusResponse {
        api_configured: configured,
        backend: backend_name.to_string(),
        message: if configured {
            format!("{} is configured and ready to use", backend_name)
        } else {
            "Claude is not configured. Either set ANTHROPIC_API_KEY environment variable or ensure 'claude' CLI (Claude Code) is installed.".to_string()
        },
    };

    Ok(ResponseJson(ApiResponse::success(response)))
}

// ============================================================================
// GSD CLI WebSocket Handler
// ============================================================================

/// WebSocket endpoint for running GSD CLI with real-time streaming
///
/// This spawns `claude /gsd:<skill>` in a PTY and streams output back to the frontend.
/// The frontend can send user input back for interactive prompts.
pub async fn gsd_cli_ws(
    ws: WebSocketUpgrade,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<GsdCliQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_path = PathBuf::from(&query.project_path);

    // Validate project path exists
    if !project_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "Project path does not exist: {}",
            query.project_path
        )));
    }

    tracing::info!(
        "GSD CLI WebSocket connection requested for path: {}, skill: {}",
        query.project_path,
        query.skill
    );

    Ok(ws.on_upgrade(move |socket| {
        handle_gsd_cli_ws(socket, deployment, project_path, query.skill, query.cols, query.rows)
    }))
}

async fn handle_gsd_cli_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    project_path: PathBuf,
    skill: String,
    cols: u16,
    rows: u16,
) {
    // Spawn the claude CLI with the GSD skill
    let command = "claude".to_string();
    let args = vec![format!("/gsd:{}", skill)];

    tracing::info!(
        "Spawning GSD CLI: {} {:?} in {:?}",
        command,
        args,
        project_path
    );

    let (session_id, mut output_rx) = match deployment
        .pty()
        .create_command_session(project_path.clone(), command, args, cols, rows)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to create GSD CLI PTY session: {}", e);
            let _ = send_gsd_error(socket, &e.to_string()).await;
            return;
        }
    };

    tracing::info!("GSD CLI session started: {}", session_id);

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Send session started message
    let started_msg = GsdCliMessage::Started {
        session_id: session_id.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&started_msg) {
        let _ = ws_sender.send(Message::Text(json.into())).await;
    }

    let pty_service = deployment.pty().clone();
    let session_id_for_input = session_id;

    // Task to forward PTY output to WebSocket
    let output_task = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            let msg = GsdCliMessage::Output {
                data: BASE64.encode(&data),
            };
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }

        // Send completion message when PTY closes
        let completed_msg = GsdCliMessage::Completed { exit_code: None };
        if let Ok(json) = serde_json::to_string(&completed_msg) {
            let _ = ws_sender.send(Message::Text(json.into())).await;
        }

        ws_sender
    });

    // Handle incoming WebSocket messages (user input, resize, abort)
    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(cmd) = serde_json::from_str::<GsdCliCommand>(&text) {
                    match cmd {
                        GsdCliCommand::Input { data } => {
                            // Decode base64 input and forward to PTY
                            if let Ok(bytes) = BASE64.decode(&data) {
                                let _ = pty_service.write(session_id_for_input, &bytes).await;
                            }
                        }
                        GsdCliCommand::Resize { cols, rows } => {
                            let _ = pty_service.resize(session_id_for_input, cols, rows).await;
                        }
                        GsdCliCommand::Abort => {
                            tracing::info!("Aborting GSD CLI session: {}", session_id_for_input);
                            break;
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Clean up
    let _ = deployment.pty().close_session(session_id).await;
    output_task.abort();
    tracing::info!("GSD CLI session closed: {}", session_id);
}

async fn send_gsd_error(mut socket: WebSocket, message: &str) -> Result<(), axum::Error> {
    let msg = GsdCliMessage::Error {
        message: message.to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap_or_default();
    socket.send(Message::Text(json.into())).await?;
    socket.close().await?;
    Ok(())
}

// ============================================================================
// Router
// ============================================================================

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    // Routes for specific session (require session to exist)
    let session_router = Router::new()
        .route("/", get(get_session).put(update_session).delete(delete_session))
        .route("/messages", get(get_messages).post(send_message))
        .route("/interactions/{interaction_id}/resolve", post(resolve_interaction))
        .route("/tasks", get(get_generated_tasks))
        .route("/tasks/approve-all", post(approve_all_tasks))
        .route("/finalize", post(finalize_session))
        .layer(from_fn_with_state(deployment.clone(), load_gsd_session_middleware));

    // Main GSD routes
    let inner = Router::new()
        .route("/status", get(get_status))
        .route("/cli/ws", get(gsd_cli_ws))  // WebSocket for full GSD CLI experience
        .route("/sessions", get(list_sessions).post(create_session))
        .nest("/sessions/{session_id}", session_router);

    Router::new().nest("/gsd", inner)
}
