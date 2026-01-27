-- GSD Sessions table for tracking interactive project creation flows
CREATE TABLE gsd_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,  -- Associated project (created after session completes)
    title TEXT NOT NULL,                                          -- User-provided title/description
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'cancelled')),
    stage TEXT NOT NULL DEFAULT 'questioning',                    -- Current stage (questioning, scoping, roadmap, phase_discussion, executing, checkpoint)
    context TEXT NOT NULL DEFAULT '{}',                           -- JSON: accumulated context from conversation
    claude_conversation_id TEXT,                                  -- Claude API conversation ID for continuity
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- GSD Messages table for storing conversation history
CREATE TABLE gsd_messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES gsd_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,                                        -- Message content (text, markdown)
    message_type TEXT NOT NULL DEFAULT 'message',                 -- Type: message, question, progress, table, code, banner, tasks_preview
    metadata TEXT NOT NULL DEFAULT '{}',                          -- JSON: type-specific data (options for questions, table data, etc.)
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- GSD Pending Interactions table for tracking what the UI needs to render
CREATE TABLE gsd_pending_interactions (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES gsd_sessions(id) ON DELETE CASCADE,
    interaction_type TEXT NOT NULL,                               -- text, single_choice, multi_choice, confirmation, scope_adjustment, phase_selection
    prompt TEXT NOT NULL,                                         -- Question/prompt to display
    options TEXT,                                                 -- JSON array of options (for choice types)
    metadata TEXT NOT NULL DEFAULT '{}',                          -- Additional metadata
    resolved BOOLEAN NOT NULL DEFAULT FALSE,                      -- Whether user has responded
    response TEXT,                                                -- User's response (JSON)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at TEXT
);

-- GSD Generated Tasks table for tasks created during the session (before finalizing to project)
CREATE TABLE gsd_generated_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES gsd_sessions(id) ON DELETE CASCADE,
    phase_number INTEGER NOT NULL,                                -- Phase number
    phase_name TEXT NOT NULL,                                     -- Phase name
    task_order INTEGER NOT NULL,                                  -- Order within phase
    title TEXT NOT NULL,
    description TEXT,
    requirements TEXT,                                            -- JSON array of requirement IDs this task addresses
    success_criteria TEXT,                                        -- JSON array of success criteria
    dependencies TEXT,                                            -- JSON array of task IDs this depends on
    approved BOOLEAN NOT NULL DEFAULT FALSE,                      -- Whether user approved this task
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes for efficient queries
CREATE INDEX idx_gsd_sessions_status ON gsd_sessions(status);
CREATE INDEX idx_gsd_sessions_project_id ON gsd_sessions(project_id);
CREATE INDEX idx_gsd_messages_session_id ON gsd_messages(session_id);
CREATE INDEX idx_gsd_messages_created_at ON gsd_messages(session_id, created_at);
CREATE INDEX idx_gsd_pending_interactions_session_id ON gsd_pending_interactions(session_id);
CREATE INDEX idx_gsd_pending_interactions_resolved ON gsd_pending_interactions(session_id, resolved);
CREATE INDEX idx_gsd_generated_tasks_session_id ON gsd_generated_tasks(session_id);
CREATE INDEX idx_gsd_generated_tasks_phase ON gsd_generated_tasks(session_id, phase_number);

-- Trigger to update updated_at on gsd_sessions
CREATE TRIGGER update_gsd_sessions_updated_at
    AFTER UPDATE ON gsd_sessions
    FOR EACH ROW
BEGIN
    UPDATE gsd_sessions SET updated_at = datetime('now') WHERE id = OLD.id;
END;
