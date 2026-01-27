-- Add task_order column to tasks table for ordering tasks within a phase
ALTER TABLE tasks ADD COLUMN task_order INTEGER;
