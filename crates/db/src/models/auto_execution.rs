use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use ts_rs::TS;
use uuid::Uuid;

#[derive(
    Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default,
)]
#[sqlx(type_name = "auto_execution_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AutoExecutionStatus {
    #[default]
    Running,
    PausedForReview,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ProjectAutoExecution {
    pub id: Uuid,
    pub project_id: Uuid,
    pub status: AutoExecutionStatus,
    pub current_phase_number: i32,
    pub current_task_id: Option<Uuid>,
    pub target_branch: String,
    pub executor_profile_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectAutoExecution {
    pub async fn create(
        pool: &SqlitePool,
        id: Uuid,
        project_id: Uuid,
        current_phase_number: i32,
        current_task_id: Uuid,
        executor_profile_id: &str,
    ) -> Result<Self, sqlx::Error> {
        let status = AutoExecutionStatus::Running;
        // target_branch is no longer used (tasks go to InReview for manual merge)
        // but kept in DB for backward compatibility
        let target_branch = "";
        sqlx::query_as!(
            ProjectAutoExecution,
            r#"INSERT INTO project_auto_executions (id, project_id, status, current_phase_number, current_task_id, target_branch, executor_profile_id)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id as "id!: Uuid", project_id as "project_id!: Uuid", status as "status!: AutoExecutionStatus", current_phase_number as "current_phase_number!: i32", current_task_id as "current_task_id: Uuid", target_branch, executor_profile_id, created_at as "created_at!: DateTime<Utc>", updated_at as "updated_at!: DateTime<Utc>""#,
            id,
            project_id,
            status,
            current_phase_number,
            current_task_id,
            target_branch,
            executor_profile_id,
        )
        .fetch_one(pool)
        .await
    }

    /// Find the active auto-execution for a project (running or paused_for_review)
    pub async fn find_active_by_project_id(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectAutoExecution,
            r#"SELECT id as "id!: Uuid", project_id as "project_id!: Uuid", status as "status!: AutoExecutionStatus", current_phase_number as "current_phase_number!: i32", current_task_id as "current_task_id: Uuid", target_branch, executor_profile_id, created_at as "created_at!: DateTime<Utc>", updated_at as "updated_at!: DateTime<Utc>"
               FROM project_auto_executions
               WHERE project_id = $1 AND status IN ('running', 'paused_for_review')
               ORDER BY created_at DESC
               LIMIT 1"#,
            project_id
        )
        .fetch_optional(pool)
        .await
    }

    /// Find running auto-execution by the task currently being executed
    pub async fn find_running_by_task_id(
        pool: &SqlitePool,
        task_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectAutoExecution,
            r#"SELECT id as "id!: Uuid", project_id as "project_id!: Uuid", status as "status!: AutoExecutionStatus", current_phase_number as "current_phase_number!: i32", current_task_id as "current_task_id: Uuid", target_branch, executor_profile_id, created_at as "created_at!: DateTime<Utc>", updated_at as "updated_at!: DateTime<Utc>"
               FROM project_auto_executions
               WHERE current_task_id = $1 AND status = 'running'
               LIMIT 1"#,
            task_id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn update_status(
        pool: &SqlitePool,
        id: Uuid,
        status: AutoExecutionStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE project_auto_executions SET status = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            id,
            status,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update_progress(
        pool: &SqlitePool,
        id: Uuid,
        current_phase_number: i32,
        current_task_id: Option<Uuid>,
        status: AutoExecutionStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE project_auto_executions
               SET current_phase_number = $2, current_task_id = $3, status = $4, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1"#,
            id,
            current_phase_number,
            current_task_id,
            status,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
