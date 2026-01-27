use std::path::Path;

use db::models::{
    auto_execution::{AutoExecutionStatus, ProjectAutoExecution},
    execution_process::{ExecutionProcess, ExecutionProcessStatus},
    merge::Merge,
    project_repo::ProjectRepo,
    repo::Repo,
    task::{Task, TaskStatus},
    workspace::{CreateWorkspace, Workspace},
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use executors::profile::ExecutorProfileId;
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

use super::{container::ContainerService, git::GitService};

#[derive(Debug, Error)]
pub enum AutoExecutionError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Git error: {0}")]
    Git(#[from] super::git::GitServiceError),
    #[error("Container error: {0}")]
    Container(#[from] super::container::ContainerError),
    #[error("Workspace error: {0}")]
    Workspace(#[from] db::models::workspace::WorkspaceError),
    #[error("No repos found for project")]
    NoRepos,
    #[error("Repo not found: {0}")]
    RepoNotFound(Uuid),
}

/// Called from `finalize_task()` after a task execution completes.
/// Returns `Ok(true)` if auto-execution handled the task (caller should skip normal finalize).
/// Returns `Ok(false)` if no auto-execution is active (caller should proceed normally).
pub async fn advance_auto_execution(
    pool: &SqlitePool,
    container: &(impl ContainerService + Sync + ?Sized),
    git: &GitService,
    task: &Task,
    workspace: &Workspace,
    execution_succeeded: bool,
) -> Result<bool, AutoExecutionError> {
    // Check if this task is part of an active auto-execution
    let auto_exec = match ProjectAutoExecution::find_running_by_task_id(pool, task.id).await? {
        Some(ae) => ae,
        None => return Ok(false), // Not in auto-execution
    };

    if !execution_succeeded {
        // Execution failed — pause auto-execution, set task to InReview for user to investigate
        tracing::warn!(
            "Auto-execution: task '{}' failed, setting auto-execution to failed",
            task.title
        );
        ProjectAutoExecution::update_status(pool, auto_exec.id, AutoExecutionStatus::Failed)
            .await?;
        Task::update_status(pool, task.id, TaskStatus::InReview).await?;
        return Ok(true);
    }

    // Auto-merge the completed task
    tracing::info!(
        "Auto-execution: auto-merging task '{}' (phase {}, order {:?})",
        task.title,
        auto_exec.current_phase_number,
        task.task_order
    );

    if let Err(e) = auto_merge_task(pool, container, git, task, workspace, &auto_exec.target_branch).await {
        tracing::error!("Auto-execution: merge failed for task '{}': {}", task.title, e);
        ProjectAutoExecution::update_status(pool, auto_exec.id, AutoExecutionStatus::Failed)
            .await?;
        Task::update_status(pool, task.id, TaskStatus::InReview).await?;
        return Ok(true);
    }

    // Find next todo task in current phase
    let next_task = find_next_todo_task(pool, task.project_id, auto_exec.current_phase_number).await?;

    if let Some(next) = next_task {
        // Start next task in same phase
        tracing::info!(
            "Auto-execution: starting next task '{}' (order {:?})",
            next.title,
            next.task_order
        );
        match start_task_for_auto_execution(
            pool,
            container,
            &next,
            &auto_exec.target_branch,
            &auto_exec.executor_profile_id,
        )
        .await
        {
            Ok(_) => {
                ProjectAutoExecution::update_progress(
                    pool,
                    auto_exec.id,
                    auto_exec.current_phase_number,
                    Some(next.id),
                    AutoExecutionStatus::Running,
                )
                .await?;
            }
            Err(e) => {
                tracing::error!(
                    "Auto-execution: failed to start next task '{}': {}",
                    next.title,
                    e
                );
                ProjectAutoExecution::update_status(
                    pool,
                    auto_exec.id,
                    AutoExecutionStatus::Failed,
                )
                .await?;
            }
        }
    } else {
        // No more tasks in this phase — pause for review
        tracing::info!(
            "Auto-execution: phase {} complete, pausing for review",
            auto_exec.current_phase_number
        );
        ProjectAutoExecution::update_progress(
            pool,
            auto_exec.id,
            auto_exec.current_phase_number,
            None,
            AutoExecutionStatus::PausedForReview,
        )
        .await?;
    }

    Ok(true)
}

/// Called from `create_phase_review` when user completes a phase review.
/// Resumes auto-execution by starting the next phase if applicable.
pub async fn resume_after_phase_review(
    pool: &SqlitePool,
    container: &(impl ContainerService + Sync + ?Sized),
    auto_exec: &ProjectAutoExecution,
) -> Result<(), AutoExecutionError> {
    // Find next phase that has todo tasks
    let next_phase_task = find_first_todo_task_in_next_phase(
        pool,
        auto_exec.project_id,
        auto_exec.current_phase_number,
    )
    .await?;

    match next_phase_task {
        Some(next) => {
            let next_phase = next.phase_number.unwrap_or(auto_exec.current_phase_number + 1);
            tracing::info!(
                "Auto-execution: resuming with phase {}, task '{}' (order {:?})",
                next_phase,
                next.title,
                next.task_order
            );

            match start_task_for_auto_execution(
                pool,
                container,
                &next,
                &auto_exec.target_branch,
                &auto_exec.executor_profile_id,
            )
            .await
            {
                Ok(_) => {
                    ProjectAutoExecution::update_progress(
                        pool,
                        auto_exec.id,
                        next_phase,
                        Some(next.id),
                        AutoExecutionStatus::Running,
                    )
                    .await?;
                }
                Err(e) => {
                    tracing::error!(
                        "Auto-execution: failed to start task '{}' in phase {}: {}",
                        next.title,
                        next_phase,
                        e
                    );
                    ProjectAutoExecution::update_status(
                        pool,
                        auto_exec.id,
                        AutoExecutionStatus::Failed,
                    )
                    .await?;
                }
            }
        }
        None => {
            // No more phases — auto-execution complete
            tracing::info!("Auto-execution: all phases completed!");
            ProjectAutoExecution::update_status(
                pool,
                auto_exec.id,
                AutoExecutionStatus::Completed,
            )
            .await?;
        }
    }

    Ok(())
}

/// Auto-merge a completed task's workspace branch into the target branch.
async fn auto_merge_task(
    pool: &SqlitePool,
    container: &(impl ContainerService + Sync + ?Sized),
    git: &GitService,
    task: &Task,
    workspace: &Workspace,
    target_branch: &str,
) -> Result<(), AutoExecutionError> {
    let workspace_repos = WorkspaceRepo::find_by_workspace_id(pool, workspace.id).await?;

    for workspace_repo in &workspace_repos {
        let repo = Repo::find_by_id(pool, workspace_repo.repo_id)
            .await?
            .ok_or(AutoExecutionError::RepoNotFound(workspace_repo.repo_id))?;

        let container_ref = container.ensure_container_exists(workspace).await?;
        let workspace_path = Path::new(&container_ref);
        let worktree_path = workspace_path.join(&repo.name);

        let task_uuid_str = task.id.to_string();
        let first_uuid_section = task_uuid_str.split('-').next().unwrap_or(&task_uuid_str);
        let mut commit_message =
            format!("{} (vibe-kanban {})", task.title, first_uuid_section);

        if let Some(description) = &task.description {
            if !description.trim().is_empty() {
                commit_message.push_str("\n\n");
                commit_message.push_str(description);
            }
        }

        let merge_commit_id = git.merge_changes(
            &repo.path,
            &worktree_path,
            &workspace.branch,
            target_branch,
            &commit_message,
        )?;

        Merge::create_direct(
            pool,
            workspace.id,
            workspace_repo.repo_id,
            target_branch,
            &merge_commit_id,
        )
        .await?;
    }

    // Mark task as done
    Task::update_status(pool, task.id, TaskStatus::Done).await?;

    // Archive workspace
    if !workspace.pinned {
        Workspace::set_archived(pool, workspace.id, true).await?;
    }

    // Stop dev servers
    let dev_servers =
        ExecutionProcess::find_running_dev_servers_by_workspace(pool, workspace.id).await?;
    for dev_server in dev_servers {
        if let Err(e) = container
            .stop_execution(&dev_server, ExecutionProcessStatus::Killed)
            .await
        {
            tracing::warn!("Auto-execution: failed to stop dev server {}: {}", dev_server.id, e);
        }
    }

    Ok(())
}

/// Find the next todo task in the given phase, ordered by task_order.
async fn find_next_todo_task(
    pool: &SqlitePool,
    project_id: Uuid,
    phase_number: i32,
) -> Result<Option<Task>, sqlx::Error> {
    Task::find_next_todo_in_phase(pool, project_id, phase_number).await
}

/// Find the first todo task in the next phase after the given one.
async fn find_first_todo_task_in_next_phase(
    pool: &SqlitePool,
    project_id: Uuid,
    current_phase_number: i32,
) -> Result<Option<Task>, sqlx::Error> {
    Task::find_first_todo_in_next_phase(pool, project_id, current_phase_number).await
}

/// Start execution for a task as part of auto-execution.
/// Creates workspace, workspace repos, and kicks off execution.
async fn start_task_for_auto_execution(
    pool: &SqlitePool,
    container: &(impl ContainerService + Sync + ?Sized),
    task: &Task,
    target_branch: &str,
    executor_profile_id_str: &str,
) -> Result<Workspace, AutoExecutionError> {
    // Get project repos
    let project_repos = ProjectRepo::find_by_project_id(pool, task.project_id).await?;
    if project_repos.is_empty() {
        return Err(AutoExecutionError::NoRepos);
    }

    // Compute agent_working_dir (same logic as create_task_attempt)
    let agent_working_dir = if project_repos.len() == 1 {
        let repo = Repo::find_by_id(pool, project_repos[0].repo_id)
            .await?
            .ok_or(AutoExecutionError::RepoNotFound(project_repos[0].repo_id))?;
        Some(repo.name)
    } else {
        None
    };

    let attempt_id = Uuid::new_v4();
    let git_branch_name = container
        .git_branch_from_workspace(&attempt_id, &task.title)
        .await;

    let workspace = Workspace::create(
        pool,
        &CreateWorkspace {
            branch: git_branch_name,
            agent_working_dir,
        },
        attempt_id,
        task.id,
    )
    .await?;

    // Create workspace repos with the auto-execution target branch
    let workspace_repos: Vec<CreateWorkspaceRepo> = project_repos
        .iter()
        .map(|pr| CreateWorkspaceRepo {
            repo_id: pr.repo_id,
            target_branch: target_branch.to_string(),
        })
        .collect();

    WorkspaceRepo::create_many(pool, workspace.id, &workspace_repos).await?;

    // Parse executor profile ID from stored JSON string
    let executor_profile_id: ExecutorProfileId = serde_json::from_str(executor_profile_id_str)
        .map_err(|e| {
            tracing::error!(
                "Failed to parse executor_profile_id '{}': {}",
                executor_profile_id_str,
                e
            );
            AutoExecutionError::NoRepos // Reuse error variant as parse failure
        })?;

    if let Err(e) = container
        .start_workspace(&workspace, executor_profile_id)
        .await
    {
        tracing::error!(
            "Auto-execution: failed to start workspace for task '{}': {}",
            task.title,
            e
        );
        return Err(AutoExecutionError::Container(e));
    }

    Ok(workspace)
}
