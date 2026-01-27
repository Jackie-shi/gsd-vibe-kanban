use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    middleware::from_fn_with_state,
    response::Json as ResponseJson,
    routing::{get, post},
};
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

/// Process Claude's response into messages, interactions, and tasks
async fn process_claude_response(
    pool: &sqlx::SqlitePool,
    session_id: Uuid,
    claude_response: &str,
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
            GsdResponseBlock::Message { content } | GsdResponseBlock::Progress { content } => {
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content,
                    message_type: GsdMessageType::Message,
                    metadata: None,
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);
            }
            GsdResponseBlock::Question { interaction_type, prompt, options } => {
                tracing::debug!("Processing Question block - type: {}, has_options: {}", interaction_type, options.is_some());

                // First create a message for the question
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: prompt.clone(),
                    message_type: GsdMessageType::Question,
                    metadata: None,
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
                    metadata: None,
                };
                let int_id = Uuid::new_v4();
                tracing::debug!("Creating pending interaction with id: {}", int_id);
                pending_interaction = Some(GsdPendingInteraction::create(pool, &int_data, int_id).await.map_err(|e| {
                    tracing::error!("Failed to create pending interaction: {}", e);
                    e
                })?);
                tracing::debug!("Pending interaction created successfully");
            }
            GsdResponseBlock::Tasks { phases } => {
                // Create message about tasks
                let msg_data = CreateGsdMessage {
                    session_id,
                    role: GsdMessageRole::Assistant,
                    content: format!("I've created {} phase(s) with tasks for your project. Please review them below.", phases.len()),
                    message_type: GsdMessageType::TasksPreview,
                    metadata: None,
                };
                let msg_id = Uuid::new_v4();
                let msg = GsdMessage::create(pool, &msg_data, msg_id).await?;
                messages.push(msg);

                // Create the tasks
                for phase in phases {
                    for (idx, task) in phase.tasks.iter().enumerate() {
                        let task_data = CreateGsdGeneratedTask {
                            session_id,
                            phase_number: phase.phase_number,
                            phase_name: phase.phase_name.clone(),
                            task_order: idx as i32 + 1,
                            title: task.title.clone(),
                            description: task.description.clone(),
                            requirements: None,
                            success_criteria: None,
                            dependencies: None,
                        };
                        let task_id = Uuid::new_v4();
                        let created_task = GsdGeneratedTask::create(pool, &task_data, task_id).await?;
                        generated_tasks.push(created_task);
                    }
                }
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
    let welcome_message = CreateGsdMessage {
        session_id: session.id,
        role: GsdMessageRole::Assistant,
        content: "Welcome! I'm here to help you plan your project. Tell me about what you want to build - describe your vision, the problem you're solving, or the product you have in mind.".to_string(),
        message_type: GsdMessageType::Message,
        metadata: None,
    };
    let msg_id = Uuid::new_v4();
    GsdMessage::create(&deployment.db().pool, &welcome_message, msg_id).await?;

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

    // 4. Process Claude's response
    let assistant_response = process_claude_response(pool, session.id, &claude_response).await?;

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

    // 5. Process Claude's response
    let response = process_claude_response(pool, session.id, &claude_response).await?;

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
pub async fn finalize_session(
    Extension(session): Extension<GsdSession>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<FinalizeSessionRequest>,
) -> Result<ResponseJson<ApiResponse<FinalizeSessionResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // 1. Get all approved tasks
    let generated_tasks = GsdGeneratedTask::find_by_session_id(pool, session.id).await?;
    let approved_tasks: Vec<_> = generated_tasks.into_iter().filter(|t| t.approved).collect();

    if approved_tasks.is_empty() {
        return Err(ApiError::BadRequest("No approved tasks to create project from".to_string()));
    }

    // 2. Create the project in the database
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

    // 3. Create tasks from approved generated tasks
    let mut tasks_created = 0;
    for gsd_task in &approved_tasks {
        let task_id = Uuid::new_v4();
        let create_task_data = CreateTask::from_title_description(
            project.id,
            gsd_task.title.clone(),
            gsd_task.description.clone(),
        );

        Task::create(pool, &create_task_data, task_id).await
            .map_err(|e| ApiError::Database(e))?;
        tasks_created += 1;
    }

    // 4. Update the GSD session with the project_id and mark as completed
    let update_data = UpdateGsdSession {
        title: None,
        status: Some(db::models::gsd_session::GsdSessionStatus::Completed),
        stage: Some("completed".to_string()),
        context: None,
        claude_conversation_id: None,
        project_id: Some(project.id),
    };
    GsdSession::update(pool, session.id, &update_data).await?;

    let response = FinalizeSessionResponse {
        project_id: project.id,
        tasks_created,
    };

    deployment
        .track_if_analytics_allowed(
            "gsd_session_finalized",
            serde_json::json!({
                "session_id": session.id.to_string(),
                "project_id": project.id.to_string(),
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
        .route("/sessions", get(list_sessions).post(create_session))
        .nest("/sessions/{session_id}", session_router);

    Router::new().nest("/gsd", inner)
}
