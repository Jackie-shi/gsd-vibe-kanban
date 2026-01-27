import { memo, useMemo, useState } from 'react';
import {
  type DragEndEvent,
  KanbanBoard,
  KanbanCards,
  KanbanHeader,
  KanbanProvider,
} from '@/components/ui/shadcn-io/kanban';
import { TaskCard } from './TaskCard';
import type { TaskStatus, TaskWithAttemptStatus, ProjectAutoExecution } from 'shared/types';
import { statusBoardColors, statusLabels } from '@/utils/statusLabels';
import { Button } from '@/components/ui/button';
import { CheckCircle2, Lock, ChevronDown, ChevronUp, Play, XCircle, Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { KanbanColumns } from './TaskKanbanBoard';

const TASK_STATUSES: TaskStatus[] = [
  'todo',
  'inprogress',
  'inreview',
  'done',
  'cancelled',
];

export interface PhaseInfo {
  phaseNumber: number;
  phaseName: string;
  tasks: TaskWithAttemptStatus[];
}

type PhaseStatus = 'active' | 'locked' | 'completed' | 'review_ready';

interface PhaseKanbanBoardProps {
  tasks: TaskWithAttemptStatus[];
  onDragEnd: (event: DragEndEvent) => void;
  onViewTaskDetails: (task: TaskWithAttemptStatus) => void;
  selectedTaskId?: string;
  onCreateTask?: () => void;
  projectId: string;
  reviewedPhases: Set<number>;
  onCompletePhaseReview: (phaseNumber: number) => void;
  searchQuery?: string;
  autoExecution?: ProjectAutoExecution | null;
  onStartAutoExecution?: () => void;
  onCancelAutoExecution?: () => void;
}

function groupTasksByPhase(tasks: TaskWithAttemptStatus[]): {
  phases: PhaseInfo[];
  unphasedTasks: TaskWithAttemptStatus[];
} {
  const phaseMap = new Map<number, PhaseInfo>();
  const unphased: TaskWithAttemptStatus[] = [];

  for (const task of tasks) {
    if (task.phase_number != null && task.phase_name != null) {
      const existing = phaseMap.get(task.phase_number);
      if (existing) {
        existing.tasks.push(task);
      } else {
        phaseMap.set(task.phase_number, {
          phaseNumber: task.phase_number,
          phaseName: task.phase_name,
          tasks: [task],
        });
      }
    } else {
      unphased.push(task);
    }
  }

  const phases = Array.from(phaseMap.values()).sort(
    (a, b) => a.phaseNumber - b.phaseNumber
  );

  return { phases, unphasedTasks: unphased };
}

function getPhaseStatus(
  phase: PhaseInfo,
  phaseIndex: number,
  allPhases: PhaseInfo[],
  reviewedPhases: Set<number>
): PhaseStatus {
  const allDone = phase.tasks.every(
    (t) => t.status === 'done' || t.status === 'cancelled'
  );
  const isReviewed = reviewedPhases.has(phase.phaseNumber);

  if (allDone && isReviewed) return 'completed';
  if (allDone && !isReviewed) return 'review_ready';

  // Check if all previous phases are completed (all done + reviewed)
  for (let i = 0; i < phaseIndex; i++) {
    const prev = allPhases[i];
    const prevAllDone = prev.tasks.every(
      (t) => t.status === 'done' || t.status === 'cancelled'
    );
    const prevReviewed = reviewedPhases.has(prev.phaseNumber);
    if (!prevAllDone || !prevReviewed) return 'locked';
  }

  return 'active';
}

function buildColumnsForPhase(
  tasks: TaskWithAttemptStatus[],
  searchQuery?: string
): KanbanColumns {
  const columns: KanbanColumns = {
    todo: [],
    inprogress: [],
    inreview: [],
    done: [],
    cancelled: [],
  };

  const normalizedSearch = searchQuery?.trim().toLowerCase();

  for (const task of tasks) {
    if (normalizedSearch) {
      const matchesTitle = task.title.toLowerCase().includes(normalizedSearch);
      const matchesDesc =
        task.description?.toLowerCase().includes(normalizedSearch) ?? false;
      if (!matchesTitle && !matchesDesc) continue;
    }
    const statusKey = task.status.toLowerCase() as TaskStatus;
    columns[statusKey]?.push(task);
  }

  // Sort tasks within each column by task_order
  for (const col of Object.values(columns)) {
    col.sort((a, b) => (a.task_order ?? 999999) - (b.task_order ?? 999999));
  }

  return columns;
}

// Phase Section component
const PhaseSection = memo(function PhaseSection({
  phase,
  phaseStatus,
  columns,
  onDragEnd,
  onViewTaskDetails,
  selectedTaskId,
  onCreateTask,
  projectId,
  onCompletePhaseReview,
}: {
  phase: PhaseInfo;
  phaseStatus: PhaseStatus;
  columns: KanbanColumns;
  onDragEnd: (event: DragEndEvent) => void;
  onViewTaskDetails: (task: TaskWithAttemptStatus) => void;
  selectedTaskId?: string;
  onCreateTask?: () => void;
  projectId: string;
  onCompletePhaseReview: (phaseNumber: number) => void;
}) {
  const [isCollapsed, setIsCollapsed] = useState(phaseStatus === 'completed');
  const isLocked = phaseStatus === 'locked';
  const isCompleted = phaseStatus === 'completed';
  const isReviewReady = phaseStatus === 'review_ready';

  const totalTasks = phase.tasks.length;
  const doneTasks = phase.tasks.filter(
    (t) => t.status === 'done' || t.status === 'cancelled'
  ).length;

  const hasVisibleTasks = Object.values(columns).some((col) => col.length > 0);

  const statusColor =
    phaseStatus === 'active'
      ? 'border-blue-500'
      : phaseStatus === 'review_ready'
        ? 'border-yellow-500'
        : phaseStatus === 'completed'
          ? 'border-green-500'
          : 'border-muted-foreground/30';

  const statusBg =
    phaseStatus === 'active'
      ? 'bg-blue-500/5'
      : phaseStatus === 'review_ready'
        ? 'bg-yellow-500/5'
        : phaseStatus === 'completed'
          ? 'bg-green-500/5'
          : 'bg-muted/30';

  return (
    <div className={cn('border-l-4 rounded-lg', statusColor, statusBg)}>
      {/* Phase Header */}
      <div
        className="flex items-center justify-between px-4 py-3 cursor-pointer select-none"
        onClick={() => setIsCollapsed(!isCollapsed)}
      >
        <div className="flex items-center gap-3">
          {isCollapsed ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronUp className="h-4 w-4 text-muted-foreground" />
          )}

          <div className="flex items-center gap-2">
            {isCompleted && (
              <CheckCircle2 className="h-5 w-5 text-green-500" />
            )}
            {isLocked && <Lock className="h-4 w-4 text-muted-foreground" />}
            <span className="font-semibold text-sm">
              Phase {phase.phaseNumber}
            </span>
            <span className="text-muted-foreground text-sm">
              {phase.phaseName}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <span className="text-xs text-muted-foreground">
            {doneTasks}/{totalTasks} tasks done
          </span>

          {/* Progress bar */}
          <div className="w-24 h-1.5 bg-muted rounded-full overflow-hidden">
            <div
              className={cn(
                'h-full rounded-full transition-all',
                isCompleted ? 'bg-green-500' : isReviewReady ? 'bg-yellow-500' : 'bg-blue-500'
              )}
              style={{
                width: `${totalTasks > 0 ? (doneTasks / totalTasks) * 100 : 0}%`,
              }}
            />
          </div>

          {isReviewReady && (
            <Button
              size="sm"
              variant="outline"
              className="text-xs border-yellow-500 text-yellow-600 hover:bg-yellow-500/10"
              onClick={(e) => {
                e.stopPropagation();
                onCompletePhaseReview(phase.phaseNumber);
              }}
            >
              <CheckCircle2 className="h-3 w-3 mr-1" />
              Complete Review
            </Button>
          )}
        </div>
      </div>

      {/* Phase Content */}
      {!isCollapsed && hasVisibleTasks && (
        <div
          className={cn(
            'w-full overflow-x-auto overflow-y-hidden',
            isLocked && 'opacity-50 pointer-events-none'
          )}
        >
          <KanbanProvider onDragEnd={onDragEnd}>
            {TASK_STATUSES.map((status) => {
              const tasks = columns[status];
              return (
                <KanbanBoard key={status} id={status}>
                  <KanbanHeader
                    name={statusLabels[status]}
                    color={statusBoardColors[status]}
                    onAddTask={!isLocked ? onCreateTask : undefined}
                  />
                  <KanbanCards>
                    {tasks.map((task, index) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        index={index}
                        status={status}
                        onViewDetails={onViewTaskDetails}
                        isOpen={selectedTaskId === task.id}
                        projectId={projectId}
                      />
                    ))}
                  </KanbanCards>
                </KanbanBoard>
              );
            })}
          </KanbanProvider>
        </div>
      )}

      {!isCollapsed && !hasVisibleTasks && (
        <div className="px-4 pb-3 text-sm text-muted-foreground italic">
          No tasks match the current search.
        </div>
      )}
    </div>
  );
});

function AutoExecutionBar({
  autoExecution,
  onStart,
  onCancel,
}: {
  autoExecution?: ProjectAutoExecution | null;
  onStart?: () => void;
  onCancel?: () => void;
}) {
  if (!autoExecution) {
    // No active auto-execution — show start button
    return (
      <div className="flex items-center gap-2 px-4 py-2 bg-muted/30 rounded-lg">
        <Button
          size="sm"
          variant="default"
          onClick={onStart}
          className="gap-1.5"
        >
          <Play className="h-3.5 w-3.5" />
          Execute All
        </Button>
        <span className="text-xs text-muted-foreground">
          Auto-execute all phases sequentially with auto-merge
        </span>
      </div>
    );
  }

  const { status, current_phase_number } = autoExecution;

  if (status === 'running') {
    return (
      <div className="flex items-center gap-3 px-4 py-2 bg-blue-500/10 border border-blue-500/30 rounded-lg">
        <Loader2 className="h-4 w-4 animate-spin text-blue-500" />
        <span className="text-sm font-medium text-blue-600">
          Executing Phase {current_phase_number}...
        </span>
        <Button
          size="sm"
          variant="outline"
          onClick={onCancel}
          className="ml-auto gap-1 text-xs"
        >
          <XCircle className="h-3 w-3" />
          Cancel
        </Button>
      </div>
    );
  }

  if (status === 'paused_for_review') {
    return (
      <div className="flex items-center gap-3 px-4 py-2 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
        <CheckCircle2 className="h-4 w-4 text-yellow-500" />
        <span className="text-sm font-medium text-yellow-600">
          Phase {current_phase_number} complete — waiting for review
        </span>
        <Button
          size="sm"
          variant="outline"
          onClick={onCancel}
          className="ml-auto gap-1 text-xs"
        >
          <XCircle className="h-3 w-3" />
          Cancel
        </Button>
      </div>
    );
  }

  if (status === 'completed') {
    return (
      <div className="flex items-center gap-3 px-4 py-2 bg-green-500/10 border border-green-500/30 rounded-lg">
        <CheckCircle2 className="h-4 w-4 text-green-500" />
        <span className="text-sm font-medium text-green-600">
          All phases executed successfully
        </span>
      </div>
    );
  }

  if (status === 'failed') {
    return (
      <div className="flex items-center gap-3 px-4 py-2 bg-destructive/10 border border-destructive/30 rounded-lg">
        <XCircle className="h-4 w-4 text-destructive" />
        <span className="text-sm font-medium text-destructive">
          Auto-execution failed at Phase {current_phase_number}
        </span>
        <Button
          size="sm"
          variant="default"
          onClick={onStart}
          className="ml-auto gap-1 text-xs"
        >
          <Play className="h-3 w-3" />
          Retry
        </Button>
      </div>
    );
  }

  // Cancelled or unknown — show start button
  return (
    <div className="flex items-center gap-2 px-4 py-2 bg-muted/30 rounded-lg">
      <Button
        size="sm"
        variant="default"
        onClick={onStart}
        className="gap-1.5"
      >
        <Play className="h-3.5 w-3.5" />
        Execute All
      </Button>
      <span className="text-xs text-muted-foreground">
        Auto-execute all phases sequentially with auto-merge
      </span>
    </div>
  );
}

function PhaseKanbanBoard({
  tasks,
  onDragEnd,
  onViewTaskDetails,
  selectedTaskId,
  onCreateTask,
  projectId,
  reviewedPhases,
  onCompletePhaseReview,
  searchQuery,
  autoExecution,
  onStartAutoExecution,
  onCancelAutoExecution,
}: PhaseKanbanBoardProps) {
  const { phases, unphasedTasks } = useMemo(
    () => groupTasksByPhase(tasks),
    [tasks]
  );

  const hasPhases = phases.length > 0;

  // If there are no phased tasks, render the traditional kanban
  if (!hasPhases) {
    return null; // Caller should fall back to regular TaskKanbanBoard
  }

  return (
    <div className="flex flex-col gap-4 p-4 w-full">
      {/* Auto-execution control bar */}
      <AutoExecutionBar
        autoExecution={autoExecution}
        onStart={onStartAutoExecution}
        onCancel={onCancelAutoExecution}
      />

      {/* Unphased tasks section (manually created tasks) */}
      {unphasedTasks.length > 0 && (
        <div className="border-l-4 border-muted-foreground/30 bg-muted/20 rounded-lg">
          <div className="px-4 py-3">
            <span className="font-semibold text-sm text-muted-foreground">
              Other Tasks
            </span>
          </div>
          <div className="w-full overflow-x-auto overflow-y-hidden">
            <KanbanProvider onDragEnd={onDragEnd}>
              {TASK_STATUSES.map((status) => {
                const filtered = unphasedTasks.filter(
                  (t) => (t.status.toLowerCase() as TaskStatus) === status
                );
                return (
                  <KanbanBoard key={status} id={status}>
                    <KanbanHeader
                      name={statusLabels[status]}
                      color={statusBoardColors[status]}
                      onAddTask={onCreateTask}
                    />
                    <KanbanCards>
                      {filtered.map((task, index) => (
                        <TaskCard
                          key={task.id}
                          task={task}
                          index={index}
                          status={status}
                          onViewDetails={onViewTaskDetails}
                          isOpen={selectedTaskId === task.id}
                          projectId={projectId}
                        />
                      ))}
                    </KanbanCards>
                  </KanbanBoard>
                );
              })}
            </KanbanProvider>
          </div>
        </div>
      )}

      {/* Phase sections */}
      {phases.map((phase, index) => {
        const phaseStatus = getPhaseStatus(
          phase,
          index,
          phases,
          reviewedPhases
        );
        const columns = buildColumnsForPhase(phase.tasks, searchQuery);

        return (
          <PhaseSection
            key={phase.phaseNumber}
            phase={phase}
            phaseStatus={phaseStatus}
            columns={columns}
            onDragEnd={onDragEnd}
            onViewTaskDetails={onViewTaskDetails}
            selectedTaskId={selectedTaskId}
            onCreateTask={onCreateTask}
            projectId={projectId}
            onCompletePhaseReview={onCompletePhaseReview}
          />
        );
      })}
    </div>
  );
}

export default memo(PhaseKanbanBoard);
