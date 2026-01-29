use axum::{
    Json, Router,
    extract::{Path, State},
    response::Json as ResponseJson,
    routing::{get, post},
};
use db::models::{
    auto_execution::{AutoExecutionStatus, ProjectAutoExecution},
    project_repo::ProjectRepo,
    repo::Repo,
    task::Task,
    workspace::CreateWorkspace,
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use deployment::Deployment;
use executors::profile::ExecutorProfileId;
use serde::{Deserialize, Serialize};
use services::services::container::ContainerService;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct StartAutoExecutionRequest {
    pub executor_profile_id: String,
}

#[axum::debug_handler]
pub async fn start_auto_execution(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<StartAutoExecutionRequest>,
) -> Result<ResponseJson<ApiResponse<ProjectAutoExecution>>, ApiError> {
    let pool = &deployment.db().pool;

    // Check no active auto-execution exists
    if let Some(existing) =
        ProjectAutoExecution::find_active_by_project_id(pool, project_id).await?
    {
        return Err(ApiError::Conflict(format!(
            "An auto-execution is already active (status: {:?})",
            existing.status
        )));
    }

    // Find the first todo task (ordered by phase_number, then task_order)
    let first_task = Task::find_first_todo_with_phase(pool, project_id).await?;

    let first_task = first_task.ok_or_else(|| {
        ApiError::BadRequest("No todo tasks with phases found to execute".to_string())
    })?;

    let phase_number = first_task.phase_number.unwrap_or(1);

    // Create auto-execution record
    let auto_exec_id = Uuid::new_v4();
    let auto_exec = ProjectAutoExecution::create(
        pool,
        auto_exec_id,
        project_id,
        phase_number,
        first_task.id,
        &payload.executor_profile_id,
    )
    .await?;

    // Start the first task execution
    let project_repos = ProjectRepo::find_by_project_id(pool, project_id).await?;
    if project_repos.is_empty() {
        ProjectAutoExecution::update_status(pool, auto_exec.id, AutoExecutionStatus::Failed)
            .await?;
        return Err(ApiError::BadRequest(
            "No repositories found for project".to_string(),
        ));
    }

    // Build workspace repos with each repo's default target branch
    let mut workspace_repos_to_create: Vec<CreateWorkspaceRepo> = Vec::new();
    let mut agent_working_dir: Option<String> = None;

    for (i, pr) in project_repos.iter().enumerate() {
        let repo = Repo::find_by_id(pool, pr.repo_id)
            .await?
            .ok_or(ApiError::BadRequest("Repo not found".to_string()))?;

        // For single repo projects, set agent_working_dir
        if project_repos.len() == 1 && i == 0 {
            agent_working_dir = Some(repo.name.clone());
        }

        // Use repo's default_target_branch or fallback to "main"
        let target_branch = repo
            .default_target_branch
            .unwrap_or_else(|| "main".to_string());

        workspace_repos_to_create.push(CreateWorkspaceRepo {
            repo_id: pr.repo_id,
            target_branch,
        });
    }

    let attempt_id = Uuid::new_v4();
    let git_branch_name = deployment
        .container()
        .git_branch_from_workspace(&attempt_id, &first_task.title)
        .await;

    let workspace = db::models::workspace::Workspace::create(
        pool,
        &CreateWorkspace {
            branch: git_branch_name,
            agent_working_dir,
        },
        attempt_id,
        first_task.id,
    )
    .await?;

    WorkspaceRepo::create_many(pool, workspace.id, &workspace_repos_to_create).await?;

    // Parse executor profile from JSON string
    let executor_profile_id: ExecutorProfileId =
        serde_json::from_str(&payload.executor_profile_id).map_err(|e| {
            ApiError::BadRequest(format!(
                "Invalid executor_profile_id '{}': {}",
                payload.executor_profile_id, e
            ))
        })?;

    if let Err(err) = deployment
        .container()
        .start_workspace(&workspace, executor_profile_id)
        .await
    {
        tracing::error!("Auto-execution: failed to start first task: {}", err);
        ProjectAutoExecution::update_status(pool, auto_exec.id, AutoExecutionStatus::Failed)
            .await?;
        return Err(ApiError::Container(err));
    }

    tracing::info!(
        "Auto-execution started for project {} — phase {}, task '{}'",
        project_id,
        phase_number,
        first_task.title
    );

    Ok(ResponseJson(ApiResponse::success(auto_exec)))
}

#[axum::debug_handler]
pub async fn cancel_auto_execution(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let pool = &deployment.db().pool;

    let auto_exec = ProjectAutoExecution::find_active_by_project_id(pool, project_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest("No active auto-execution found".to_string()))?;

    ProjectAutoExecution::update_status(pool, auto_exec.id, AutoExecutionStatus::Cancelled)
        .await?;

    tracing::info!(
        "Auto-execution cancelled for project {}",
        project_id
    );

    Ok(ResponseJson(ApiResponse::success(())))
}

#[axum::debug_handler]
pub async fn get_auto_execution_status(
    Path(project_id): Path<Uuid>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Option<ProjectAutoExecution>>>, ApiError> {
    let pool = &deployment.db().pool;
    let auto_exec = ProjectAutoExecution::find_active_by_project_id(pool, project_id).await?;
    Ok(ResponseJson(ApiResponse::success(auto_exec)))
}

pub fn router(_deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let inner = Router::new()
        .route("/auto-execution/start", post(start_auto_execution))
        .route("/auto-execution/cancel", post(cancel_auto_execution))
        .route("/auto-execution/status", get(get_auto_execution_status));

    Router::new().nest("/projects/{project_id}", inner)
}
