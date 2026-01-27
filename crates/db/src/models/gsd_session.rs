use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use ts_rs::TS;
use uuid::Uuid;

// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum GsdSessionStatus {
    #[default]
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum GsdMessageRole {
    User,
    #[default]
    Assistant,
    System,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GsdMessageType {
    #[default]
    Message,
    Question,
    Progress,
    Table,
    Code,
    Banner,
    TasksPreview,
    ScopeBoard,
    RoadmapTimeline,
    StatusIndicator,
    // New types for full GSD workflow
    ResearchSummary,
    Requirements,
    Roadmap,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GsdInteractionType {
    #[default]
    Text,
    SingleChoice,
    MultiChoice,
    Confirmation,
    ScopeAdjustment,
    PhaseSelection,
}

// ============================================================================
// Models
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct GsdSession {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub status: GsdSessionStatus,
    pub stage: String,
    pub context: String, // JSON string
    pub claude_conversation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct GsdMessage {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: GsdMessageRole,
    pub content: String,
    pub message_type: GsdMessageType,
    pub metadata: String, // JSON string
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct GsdPendingInteraction {
    pub id: Uuid,
    pub session_id: Uuid,
    pub interaction_type: GsdInteractionType,
    pub prompt: String,
    pub options: Option<String>, // JSON array
    pub metadata: String,        // JSON string
    pub resolved: bool,
    pub response: Option<String>, // JSON
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct GsdGeneratedTask {
    pub id: Uuid,
    pub session_id: Uuid,
    pub phase_number: i32,
    pub phase_name: String,
    pub task_order: i32,
    pub title: String,
    pub description: Option<String>,
    pub requirements: Option<String>,      // JSON array
    pub success_criteria: Option<String>,  // JSON array
    pub dependencies: Option<String>,      // JSON array of task IDs
    pub approved: bool,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// DTOs
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateGsdSession {
    pub title: String,
    pub repositories: Option<Vec<String>>, // Optional: pre-selected repository paths
    pub project_path: Option<String>,      // Optional: project directory path for GSD
    pub project_id: Option<Uuid>,          // Optional: link to existing project
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UpdateGsdSession {
    pub title: Option<String>,
    pub status: Option<GsdSessionStatus>,
    pub stage: Option<String>,
    pub context: Option<String>,
    pub claude_conversation_id: Option<String>,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateGsdMessage {
    pub session_id: Uuid,
    pub role: GsdMessageRole,
    pub content: String,
    pub message_type: GsdMessageType,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GsdInteractionOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateGsdPendingInteraction {
    pub session_id: Uuid,
    pub interaction_type: GsdInteractionType,
    pub prompt: String,
    pub options: Option<Vec<GsdInteractionOption>>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ResolveGsdInteraction {
    pub response: GsdUserInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateGsdGeneratedTask {
    pub session_id: Uuid,
    pub phase_number: i32,
    pub phase_name: String,
    pub task_order: i32,
    pub title: String,
    pub description: Option<String>,
    pub requirements: Option<Vec<String>>,
    pub success_criteria: Option<Vec<String>>,
    pub dependencies: Option<Vec<Uuid>>,
}

/// User input for resolving an interaction
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum GsdUserInput {
    Text(String),
    SingleChoice(String),
    MultiChoice(Vec<String>),
    Confirmation(bool),
}

/// Full session state for frontend rendering
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GsdSessionState {
    pub session: GsdSession,
    pub messages: Vec<GsdMessage>,
    pub pending_interaction: Option<GsdPendingInteraction>,
    pub generated_tasks: Vec<GsdGeneratedTask>,
}

// ============================================================================
// Implementations
// ============================================================================

impl GsdSession {
    pub async fn find_all(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdSession,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id: Uuid",
                title,
                status as "status!: GsdSessionStatus",
                stage,
                context,
                claude_conversation_id,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
            FROM gsd_sessions
            ORDER BY created_at DESC"#
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_active(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdSession,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id: Uuid",
                title,
                status as "status!: GsdSessionStatus",
                stage,
                context,
                claude_conversation_id,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
            FROM gsd_sessions
            WHERE status = 'active'
            ORDER BY created_at DESC"#
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdSession,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id: Uuid",
                title,
                status as "status!: GsdSessionStatus",
                stage,
                context,
                claude_conversation_id,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
            FROM gsd_sessions
            WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn create(pool: &SqlitePool, data: &CreateGsdSession, id: Uuid) -> Result<Self, sqlx::Error> {
        // First, create the session with title only
        let session = sqlx::query_as!(
            GsdSession,
            r#"INSERT INTO gsd_sessions (id, title)
            VALUES ($1, $2)
            RETURNING
                id as "id!: Uuid",
                project_id as "project_id: Uuid",
                title,
                status as "status!: GsdSessionStatus",
                stage,
                context,
                claude_conversation_id,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>""#,
            id,
            data.title
        )
        .fetch_one(pool)
        .await?;

        // If project_id is provided, update the session to link it
        if let Some(project_id) = data.project_id {
            let update = UpdateGsdSession {
                title: None,
                status: None,
                stage: None,
                context: None,
                claude_conversation_id: None,
                project_id: Some(project_id),
            };
            return Self::update(pool, id, &update).await;
        }

        Ok(session)
    }

    pub async fn update(pool: &SqlitePool, id: Uuid, data: &UpdateGsdSession) -> Result<Self, sqlx::Error> {
        // Build dynamic update - for simplicity, we'll update all provided fields
        let current = Self::find_by_id(pool, id).await?.ok_or(sqlx::Error::RowNotFound)?;

        let title = data.title.clone().unwrap_or(current.title);
        let status = data.status.clone().unwrap_or(current.status);
        let stage = data.stage.clone().unwrap_or(current.stage);
        let context = data.context.clone().unwrap_or(current.context);
        let claude_conversation_id = data.claude_conversation_id.clone().or(current.claude_conversation_id);
        let project_id = data.project_id.or(current.project_id);

        sqlx::query_as!(
            GsdSession,
            r#"UPDATE gsd_sessions
            SET title = $2, status = $3, stage = $4, context = $5, claude_conversation_id = $6, project_id = $7
            WHERE id = $1
            RETURNING
                id as "id!: Uuid",
                project_id as "project_id: Uuid",
                title,
                status as "status!: GsdSessionStatus",
                stage,
                context,
                claude_conversation_id,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>""#,
            id,
            title,
            status,
            stage,
            context,
            claude_conversation_id,
            project_id
        )
        .fetch_one(pool)
        .await
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!("DELETE FROM gsd_sessions WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Get full session state including messages and pending interactions
    pub async fn get_state(pool: &SqlitePool, id: Uuid) -> Result<Option<GsdSessionState>, sqlx::Error> {
        let session = match Self::find_by_id(pool, id).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        let messages = GsdMessage::find_by_session_id(pool, id).await?;
        let pending_interaction = GsdPendingInteraction::find_pending_by_session_id(pool, id).await?;
        let generated_tasks = GsdGeneratedTask::find_by_session_id(pool, id).await?;

        Ok(Some(GsdSessionState {
            session,
            messages,
            pending_interaction,
            generated_tasks,
        }))
    }
}

impl GsdMessage {
    pub async fn find_by_session_id(pool: &SqlitePool, session_id: Uuid) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdMessage,
            r#"SELECT
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                role as "role!: GsdMessageRole",
                content,
                message_type as "message_type!: GsdMessageType",
                metadata,
                created_at as "created_at!: DateTime<Utc>"
            FROM gsd_messages
            WHERE session_id = $1
            ORDER BY created_at ASC"#,
            session_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create(pool: &SqlitePool, data: &CreateGsdMessage, id: Uuid) -> Result<Self, sqlx::Error> {
        let metadata = data.metadata.clone().unwrap_or_else(|| "{}".to_string());

        sqlx::query_as!(
            GsdMessage,
            r#"INSERT INTO gsd_messages (id, session_id, role, content, message_type, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                role as "role!: GsdMessageRole",
                content,
                message_type as "message_type!: GsdMessageType",
                metadata,
                created_at as "created_at!: DateTime<Utc>""#,
            id,
            data.session_id,
            data.role,
            data.content,
            data.message_type,
            metadata
        )
        .fetch_one(pool)
        .await
    }
}

impl GsdPendingInteraction {
    pub async fn find_by_session_id(pool: &SqlitePool, session_id: Uuid) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdPendingInteraction,
            r#"SELECT
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                interaction_type as "interaction_type!: GsdInteractionType",
                prompt,
                options,
                metadata,
                resolved as "resolved!: bool",
                response,
                created_at as "created_at!: DateTime<Utc>",
                resolved_at as "resolved_at: DateTime<Utc>"
            FROM gsd_pending_interactions
            WHERE session_id = $1
            ORDER BY created_at ASC"#,
            session_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_pending_by_session_id(pool: &SqlitePool, session_id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdPendingInteraction,
            r#"SELECT
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                interaction_type as "interaction_type!: GsdInteractionType",
                prompt,
                options,
                metadata,
                resolved as "resolved!: bool",
                response,
                created_at as "created_at!: DateTime<Utc>",
                resolved_at as "resolved_at: DateTime<Utc>"
            FROM gsd_pending_interactions
            WHERE session_id = $1 AND resolved = FALSE
            ORDER BY created_at DESC
            LIMIT 1"#,
            session_id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdPendingInteraction,
            r#"SELECT
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                interaction_type as "interaction_type!: GsdInteractionType",
                prompt,
                options,
                metadata,
                resolved as "resolved!: bool",
                response,
                created_at as "created_at!: DateTime<Utc>",
                resolved_at as "resolved_at: DateTime<Utc>"
            FROM gsd_pending_interactions
            WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn create(pool: &SqlitePool, data: &CreateGsdPendingInteraction, id: Uuid) -> Result<Self, sqlx::Error> {
        let options = data.options.as_ref().map(|o| serde_json::to_string(o).unwrap_or_default());
        let metadata = data.metadata.clone().unwrap_or_else(|| "{}".to_string());

        sqlx::query_as!(
            GsdPendingInteraction,
            r#"INSERT INTO gsd_pending_interactions (id, session_id, interaction_type, prompt, options, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                interaction_type as "interaction_type!: GsdInteractionType",
                prompt,
                options,
                metadata,
                resolved as "resolved!: bool",
                response,
                created_at as "created_at!: DateTime<Utc>",
                resolved_at as "resolved_at: DateTime<Utc>""#,
            id,
            data.session_id,
            data.interaction_type,
            data.prompt,
            options,
            metadata
        )
        .fetch_one(pool)
        .await
    }

    pub async fn resolve(pool: &SqlitePool, id: Uuid, response: &GsdUserInput) -> Result<Self, sqlx::Error> {
        let response_str = serde_json::to_string(response).unwrap_or_default();

        sqlx::query_as!(
            GsdPendingInteraction,
            r#"UPDATE gsd_pending_interactions
            SET resolved = TRUE, response = $2, resolved_at = datetime('now')
            WHERE id = $1
            RETURNING
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                interaction_type as "interaction_type!: GsdInteractionType",
                prompt,
                options,
                metadata,
                resolved as "resolved!: bool",
                response,
                created_at as "created_at!: DateTime<Utc>",
                resolved_at as "resolved_at: DateTime<Utc>""#,
            id,
            response_str
        )
        .fetch_one(pool)
        .await
    }
}

impl GsdGeneratedTask {
    pub async fn find_by_session_id(pool: &SqlitePool, session_id: Uuid) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            GsdGeneratedTask,
            r#"SELECT
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                phase_number as "phase_number!: i32",
                phase_name,
                task_order as "task_order!: i32",
                title,
                description,
                requirements,
                success_criteria,
                dependencies,
                approved as "approved!: bool",
                created_at as "created_at!: DateTime<Utc>"
            FROM gsd_generated_tasks
            WHERE session_id = $1
            ORDER BY phase_number ASC, task_order ASC"#,
            session_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create(pool: &SqlitePool, data: &CreateGsdGeneratedTask, id: Uuid) -> Result<Self, sqlx::Error> {
        let requirements = data.requirements.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default());
        let success_criteria = data.success_criteria.as_ref().map(|s| serde_json::to_string(s).unwrap_or_default());
        let dependencies = data.dependencies.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());

        sqlx::query_as!(
            GsdGeneratedTask,
            r#"INSERT INTO gsd_generated_tasks (id, session_id, phase_number, phase_name, task_order, title, description, requirements, success_criteria, dependencies)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                phase_number as "phase_number!: i32",
                phase_name,
                task_order as "task_order!: i32",
                title,
                description,
                requirements,
                success_criteria,
                dependencies,
                approved as "approved!: bool",
                created_at as "created_at!: DateTime<Utc>""#,
            id,
            data.session_id,
            data.phase_number,
            data.phase_name,
            data.task_order,
            data.title,
            data.description,
            requirements,
            success_criteria,
            dependencies
        )
        .fetch_one(pool)
        .await
    }

    pub async fn approve(pool: &SqlitePool, id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            GsdGeneratedTask,
            r#"UPDATE gsd_generated_tasks
            SET approved = TRUE
            WHERE id = $1
            RETURNING
                id as "id!: Uuid",
                session_id as "session_id!: Uuid",
                phase_number as "phase_number!: i32",
                phase_name,
                task_order as "task_order!: i32",
                title,
                description,
                requirements,
                success_criteria,
                dependencies,
                approved as "approved!: bool",
                created_at as "created_at!: DateTime<Utc>""#,
            id
        )
        .fetch_one(pool)
        .await
    }

    pub async fn approve_all_by_session(pool: &SqlitePool, session_id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!(
            "UPDATE gsd_generated_tasks SET approved = TRUE WHERE session_id = $1",
            session_id
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete_by_session_id(pool: &SqlitePool, session_id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!(
            "DELETE FROM gsd_generated_tasks WHERE session_id = $1",
            session_id
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}
