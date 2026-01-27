-- Auto-execution tracking for sequential phase-based task execution
CREATE TABLE project_auto_executions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'paused_for_review', 'completed', 'cancelled', 'failed')),
    current_phase_number INTEGER NOT NULL,
    current_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    target_branch TEXT NOT NULL,
    executor_profile_id TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_pae_project_status ON project_auto_executions(project_id, status);
