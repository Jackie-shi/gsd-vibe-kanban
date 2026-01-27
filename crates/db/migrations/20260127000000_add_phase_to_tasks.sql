-- Add phase tracking fields to tasks table for GSD multi-phase workflow
ALTER TABLE tasks ADD COLUMN phase_number INTEGER;
ALTER TABLE tasks ADD COLUMN phase_name TEXT;

-- Index for efficient phase-based queries
CREATE INDEX idx_tasks_phase ON tasks(project_id, phase_number);

-- Track phase reviews: user must explicitly review each phase before the next unlocks
CREATE TABLE project_phase_reviews (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    phase_number INTEGER NOT NULL,
    reviewed_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id, phase_number)
);

CREATE INDEX idx_project_phase_reviews ON project_phase_reviews(project_id, phase_number);
