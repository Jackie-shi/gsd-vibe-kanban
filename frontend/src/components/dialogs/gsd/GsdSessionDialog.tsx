import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle, Send, Check, Loader2, Sparkles, BookOpen, FileText, Map, CheckCircle2 } from 'lucide-react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import {
  gsdApi,
  GsdSessionState,
  GsdMessage,
  GsdPendingInteraction,
  GsdInteractionOption,
  GsdGeneratedTask,
  GsdStatusResponse,
} from '@/lib/api';
import { cn } from '@/lib/utils';

// Helper to parse metadata safely
function parseMetadata(metadata: string): Record<string, unknown> {
  try {
    return JSON.parse(metadata);
  } catch {
    return {};
  }
}

export interface GsdSessionDialogProps {
  existingSessionId?: string;
  projectId?: string;
  projectPath?: string;
}

export type GsdSessionDialogResult =
  | { status: 'completed'; projectId: string }
  | { status: 'cancelled' };

// ============================================================================
// Subcomponents
// ============================================================================

// Stage indicator component
const StageIndicator = ({ stage }: { stage: string }) => {
  const stageInfo: Record<string, { label: string; color: string }> = {
    vision: { label: 'Vision & Goals', color: 'text-blue-600 dark:text-blue-400' },
    users: { label: 'Users & Use Cases', color: 'text-green-600 dark:text-green-400' },
    technical: { label: 'Technical Context', color: 'text-purple-600 dark:text-purple-400' },
    features: { label: 'Feature Discovery', color: 'text-orange-600 dark:text-orange-400' },
    research: { label: 'Research', color: 'text-cyan-600 dark:text-cyan-400' },
    requirements: { label: 'Requirements', color: 'text-pink-600 dark:text-pink-400' },
    roadmap: { label: 'Roadmap', color: 'text-yellow-600 dark:text-yellow-400' },
    tasks: { label: 'Task Generation', color: 'text-emerald-600 dark:text-emerald-400' },
  };

  const info = stageInfo[stage] || { label: stage, color: 'text-muted-foreground' };

  return (
    <span className={cn('text-xs font-medium uppercase tracking-wide', info.color)}>
      {info.label}
    </span>
  );
};

// Research Summary Card component
const ResearchSummaryCard = ({ metadata }: { metadata: Record<string, unknown> }) => {
  const findings = (metadata.findings || []) as { category: string; items: string[] }[];
  const recommendations = (metadata.recommendations || '') as string;

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-cyan-600 dark:text-cyan-400">
        <BookOpen className="h-5 w-5" />
        <span className="font-semibold">Research Summary</span>
      </div>

      {findings.map((finding, idx) => (
        <div key={idx} className="bg-background/50 rounded-lg p-3">
          <div className="font-medium text-sm mb-2">{finding.category}</div>
          <ul className="space-y-1">
            {finding.items.map((item, itemIdx) => (
              <li key={itemIdx} className="text-sm text-muted-foreground flex items-start gap-2">
                <CheckCircle2 className="h-3 w-3 mt-1 flex-shrink-0 text-cyan-500" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
      ))}

      {recommendations && (
        <div className="bg-cyan-500/10 rounded-lg p-3 border border-cyan-500/20">
          <div className="font-medium text-sm mb-1 text-cyan-700 dark:text-cyan-300">💡 Recommendations</div>
          <div className="text-sm">{recommendations}</div>
        </div>
      )}
    </div>
  );
};

// Requirements Card component
const RequirementsCard = ({ metadata }: { metadata: Record<string, unknown> }) => {
  const functional = (metadata.functional || []) as { id: string; title: string; description: string; priority: string; user_stories: string[] }[];
  const nonFunctional = (metadata.non_functional || []) as { id: string; category: string; requirement: string; acceptance_criteria?: string }[];

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-pink-600 dark:text-pink-400">
        <FileText className="h-5 w-5" />
        <span className="font-semibold">Requirements</span>
      </div>

      {functional.length > 0 && (
        <div>
          <div className="text-sm font-medium mb-2">Functional Requirements</div>
          <div className="space-y-2">
            {functional.map((req, idx) => (
              <div key={idx} className="bg-background/50 rounded-lg p-3 border-l-2 border-pink-500">
                <div className="flex items-center gap-2 justify-between">
                  <span className="font-mono text-xs text-pink-600">{req.id}</span>
                  <span className={cn(
                    'text-xs px-2 py-0.5 rounded-full',
                    req.priority === 'must-have' && 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400',
                    req.priority === 'should-have' && 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400',
                    req.priority === 'nice-to-have' && 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400',
                  )}>{req.priority}</span>
                </div>
                <div className="font-medium text-sm mt-1">{req.title}</div>
                <div className="text-xs text-muted-foreground mt-1">{req.description}</div>
                {req.user_stories && req.user_stories.length > 0 && (
                  <div className="mt-2 text-xs italic text-muted-foreground">
                    {req.user_stories[0]}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {nonFunctional.length > 0 && (
        <div>
          <div className="text-sm font-medium mb-2">Non-Functional Requirements</div>
          <div className="space-y-2">
            {nonFunctional.map((req, idx) => (
              <div key={idx} className="bg-background/50 rounded-lg p-3 border-l-2 border-purple-500">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-xs text-purple-600">{req.id}</span>
                  <span className="text-xs bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400 px-2 py-0.5 rounded-full">{req.category}</span>
                </div>
                <div className="text-sm mt-1">{req.requirement}</div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

// Roadmap Card component
const RoadmapCard = ({ metadata }: { metadata: Record<string, unknown> }) => {
  const milestones = (metadata.milestones || []) as { phase: number; name: string; goal: string; success_criteria: string[]; estimated_tasks?: number }[];

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400">
        <Map className="h-5 w-5" />
        <span className="font-semibold">Project Roadmap</span>
      </div>

      <div className="space-y-3">
        {milestones.map((milestone, idx) => (
          <div key={idx} className="bg-background/50 rounded-lg p-4 border-l-4 border-yellow-500">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-full bg-yellow-500/20 flex items-center justify-center text-yellow-600 font-bold text-sm">
                {milestone.phase}
              </div>
              <div>
                <div className="font-semibold">{milestone.name}</div>
                <div className="text-sm text-muted-foreground">{milestone.goal}</div>
              </div>
            </div>

            {milestone.success_criteria && milestone.success_criteria.length > 0 && (
              <div className="mt-3 pl-11">
                <div className="text-xs font-medium text-muted-foreground mb-1">Success Criteria:</div>
                <ul className="space-y-1">
                  {milestone.success_criteria.map((criteria, cIdx) => (
                    <li key={cIdx} className="text-xs text-muted-foreground flex items-start gap-2">
                      <CheckCircle2 className="h-3 w-3 mt-0.5 flex-shrink-0 text-yellow-500" />
                      <span>{criteria}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {milestone.estimated_tasks && (
              <div className="mt-2 pl-11 text-xs text-muted-foreground">
                ~{milestone.estimated_tasks} tasks
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

// Message display component
const MessageBubble = ({ message }: { message: GsdMessage }) => {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  const metadata = useMemo(() => parseMetadata(message.metadata), [message.metadata]);
  const stage = metadata.stage as string | undefined;

  // Check if this is a special message type with structured data
  const isResearchSummary = message.message_type === 'research_summary';
  const isRequirements = message.message_type === 'requirements';
  const isRoadmap = message.message_type === 'roadmap';
  const isStructuredMessage = isResearchSummary || isRequirements || isRoadmap;

  return (
    <div
      className={cn(
        'flex w-full mb-4',
        isUser ? 'justify-end' : 'justify-start'
      )}
    >
      {/* User message - orange border and shadow */}
      {isUser && (
        <div className="flex items-start gap-3 max-w-[85%]">
          <div
            className={cn(
              'rounded-2xl px-5 py-3',
              'bg-white dark:bg-zinc-900',
              'border-2 border-orange-400',
              'shadow-[0_2px_12px_rgba(251,146,60,0.25)]'
            )}
          >
            <div className="flex items-center gap-2 mb-1">
              <div className="w-2 h-2 rounded-full bg-orange-400" />
              <span className="text-xs font-semibold uppercase tracking-wide text-orange-600 dark:text-orange-400">You</span>
            </div>
            <div className="whitespace-pre-wrap text-sm leading-relaxed text-foreground">{message.content}</div>
          </div>
        </div>
      )}

      {/* Assistant/System message */}
      {!isUser && (
        <div
          className={cn(
            'rounded-2xl px-5 py-3',
            isSystem
              ? 'bg-muted/50 text-muted-foreground italic border border-muted max-w-[85%]'
              : isStructuredMessage
                ? 'bg-muted/80 border border-border w-full max-w-[95%]'
                : 'bg-muted/80 border border-border max-w-[85%]'
          )}
        >
          {!isSystem && (
            <div className="flex items-center gap-2 mb-2">
              <div className="w-2 h-2 rounded-full bg-gray-400" />
              <span className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">GSD Assistant</span>
              {stage && (
                <>
                  <span className="text-muted-foreground">•</span>
                  <StageIndicator stage={stage} />
                </>
              )}
            </div>
          )}

          {/* Render based on message type */}
          {message.message_type === 'banner' ? (
            <div className="font-mono text-center py-2 border-y border-current/20 my-1">
              <div className="text-xs opacity-60">GSD</div>
              <div className="font-bold">{message.content}</div>
            </div>
          ) : message.message_type === 'code' ? (
            <pre className="font-mono text-sm bg-black/10 p-2 rounded overflow-x-auto">
              {message.content}
            </pre>
          ) : message.message_type === 'progress' ? (
            <div className="flex items-center gap-2">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>{message.content}</span>
            </div>
          ) : isResearchSummary ? (
            <ResearchSummaryCard metadata={metadata} />
          ) : isRequirements ? (
            <RequirementsCard metadata={metadata} />
          ) : isRoadmap ? (
            <RoadmapCard metadata={metadata} />
          ) : (
            <div className="whitespace-pre-wrap text-sm leading-relaxed">{message.content}</div>
          )}
        </div>
      )}
    </div>
  );
};

// Interaction renderer - dynamically renders based on interaction type
const InteractionRenderer = ({
  interaction,
  onResolve,
  isLoading,
}: {
  interaction: GsdPendingInteraction;
  onResolve: (value: unknown, displayText: string) => void;
  isLoading: boolean;
}) => {
  const [textValue, setTextValue] = useState('');
  const [selectedValues, setSelectedValues] = useState<string[]>([]);

  const options: GsdInteractionOption[] = interaction.options
    ? JSON.parse(interaction.options)
    : [];

  const handleSingleChoice = (value: string, label: string) => {
    onResolve(value, `✓ ${label}`);
  };

  const handleMultiChoice = () => {
    const selectedLabels = options
      .filter((opt) => selectedValues.includes(opt.value))
      .map((opt) => opt.label);
    onResolve(selectedValues, `✓ Selected: ${selectedLabels.join(', ')}`);
  };

  const handleTextSubmit = () => {
    if (textValue.trim()) {
      onResolve(textValue.trim(), textValue.trim());
    }
  };

  const handleConfirmation = (confirmed: boolean) => {
    onResolve(confirmed, confirmed ? '✓ Yes' : '✗ No');
  };

  const toggleMultiChoice = (value: string) => {
    setSelectedValues((prev) =>
      prev.includes(value)
        ? prev.filter((v) => v !== value)
        : [...prev, value]
    );
  };

  return (
    <div className="border rounded-lg p-4 bg-muted/30 mb-4">
      <p className="font-medium mb-3">{interaction.prompt}</p>

      {interaction.interaction_type === 'text' && (
        <div className="space-y-2">
          <Textarea
            value={textValue}
            onChange={(e) => setTextValue(e.target.value)}
            placeholder="Type your response..."
            className="min-h-[100px]"
            disabled={isLoading}
          />
          <Button
            onClick={handleTextSubmit}
            disabled={!textValue.trim() || isLoading}
            className="w-full"
          >
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
            ) : (
              <Send className="h-4 w-4 mr-2" />
            )}
            Submit
          </Button>
        </div>
      )}

      {interaction.interaction_type === 'single_choice' && (
        <div className="space-y-2">
          {options.map((option) => (
            <button
              key={option.value}
              onClick={() => handleSingleChoice(option.value, option.label)}
              disabled={isLoading}
              className={cn(
                'w-full text-left p-3 rounded-lg border transition-colors',
                'hover:bg-accent hover:border-primary',
                'disabled:opacity-50 disabled:cursor-not-allowed'
              )}
            >
              <div className="font-medium">{option.label}</div>
              {option.description && (
                <div className="text-sm text-muted-foreground mt-1">
                  {option.description}
                </div>
              )}
            </button>
          ))}
        </div>
      )}

      {interaction.interaction_type === 'multi_choice' && (
        <div className="space-y-2">
          {options.map((option) => (
            <button
              key={option.value}
              onClick={() => toggleMultiChoice(option.value)}
              disabled={isLoading}
              className={cn(
                'w-full text-left p-3 rounded-lg border transition-colors flex items-start gap-3',
                selectedValues.includes(option.value)
                  ? 'bg-primary/10 border-primary'
                  : 'hover:bg-accent',
                'disabled:opacity-50 disabled:cursor-not-allowed'
              )}
            >
              <div
                className={cn(
                  'mt-0.5 h-5 w-5 rounded border flex items-center justify-center',
                  selectedValues.includes(option.value)
                    ? 'bg-primary border-primary text-primary-foreground'
                    : 'border-input'
                )}
              >
                {selectedValues.includes(option.value) && (
                  <Check className="h-3 w-3" />
                )}
              </div>
              <div className="flex-1">
                <div className="font-medium">{option.label}</div>
                {option.description && (
                  <div className="text-sm text-muted-foreground mt-1">
                    {option.description}
                  </div>
                )}
              </div>
            </button>
          ))}
          <Button
            onClick={handleMultiChoice}
            disabled={selectedValues.length === 0 || isLoading}
            className="w-full mt-2"
          >
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
            ) : (
              <Check className="h-4 w-4 mr-2" />
            )}
            Confirm Selection ({selectedValues.length})
          </Button>
        </div>
      )}

      {interaction.interaction_type === 'confirmation' && (
        <div className="flex gap-2">
          <Button
            onClick={() => handleConfirmation(true)}
            disabled={isLoading}
            className="flex-1"
          >
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
            ) : (
              <Check className="h-4 w-4 mr-2" />
            )}
            Yes
          </Button>
          <Button
            onClick={() => handleConfirmation(false)}
            disabled={isLoading}
            variant="outline"
            className="flex-1"
          >
            No
          </Button>
        </div>
      )}

      {interaction.interaction_type === 'phase_selection' && (
        <div className="space-y-2">
          <input
            type="number"
            min="1"
            value={textValue}
            onChange={(e) => setTextValue(e.target.value)}
            placeholder="Enter phase number..."
            className="w-full p-2 border rounded"
            disabled={isLoading}
          />
          <Button
            onClick={() => {
              if (textValue.trim()) {
                onResolve(textValue.trim(), `✓ Selected Phase ${textValue.trim()}`);
              }
            }}
            disabled={!textValue.trim() || isLoading}
            className="w-full"
          >
            Select Phase
          </Button>
        </div>
      )}
    </div>
  );
};

// Tasks preview component
const TasksPreview = ({ tasks }: { tasks: GsdGeneratedTask[] }) => {
  if (tasks.length === 0) return null;

  // Group tasks by phase
  const phaseGroups = tasks.reduce(
    (acc, task) => {
      const key = `${task.phase_number}-${task.phase_name}`;
      if (!acc[key]) {
        acc[key] = { phase_number: task.phase_number, phase_name: task.phase_name, tasks: [] };
      }
      acc[key].tasks.push(task);
      return acc;
    },
    {} as Record<string, { phase_number: number; phase_name: string; tasks: GsdGeneratedTask[] }>
  );

  return (
    <div className="border rounded-lg p-4 bg-muted/30 mb-4">
      <h3 className="font-semibold mb-3 flex items-center gap-2">
        <Sparkles className="h-4 w-4" />
        Generated Tasks ({tasks.length})
      </h3>
      <div className="space-y-4">
        {Object.values(phaseGroups).map((group) => (
          <div key={`${group.phase_number}-${group.phase_name}`}>
            <div className="text-sm font-medium text-muted-foreground mb-2">
              Phase {group.phase_number}: {group.phase_name}
            </div>
            <div className="space-y-1 pl-4 border-l-2 border-muted">
              {group.tasks.map((task) => (
                <div
                  key={task.id}
                  className={cn(
                    'p-2 rounded text-sm',
                    task.approved ? 'bg-green-500/10' : 'bg-muted/50'
                  )}
                >
                  <div className="font-medium">{task.title}</div>
                  {task.description && (
                    <div className="text-muted-foreground text-xs mt-1">
                      {task.description}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

// ============================================================================
// Main Dialog
// ============================================================================

const GsdSessionDialogImpl = NiceModal.create<GsdSessionDialogProps>(
  ({ existingSessionId, projectId, projectPath }) => {
    const modal = useModal();
    const [sessionState, setSessionState] = useState<GsdSessionState | null>(null);
    const [apiStatus, setApiStatus] = useState<GsdStatusResponse | null>(null);
    const [inputValue, setInputValue] = useState('');
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const messagesEndRef = useRef<HTMLDivElement>(null);

    // Track if we're linked to an existing project (tasks will be added to it)
    const isLinkedToExistingProject = Boolean(projectId);

    // Auto-scroll to bottom when messages change
    useEffect(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [sessionState?.messages]);

    // Check API status on mount
    useEffect(() => {
      const checkStatus = async () => {
        try {
          const status = await gsdApi.getStatus();
          setApiStatus(status);
        } catch (err) {
          console.error('Failed to check GSD API status:', err);
        }
      };

      if (modal.visible) {
        checkStatus();
      }
    }, [modal.visible]);

    // Initialize session
    useEffect(() => {
      const initSession = async () => {
        setIsLoading(true);
        setError(null);
        try {
          if (existingSessionId) {
            const state = await gsdApi.getSession(existingSessionId);
            setSessionState(state);
          } else {
            // Pass projectId when creating session so tasks will be added to the existing project
            const state = await gsdApi.createSession('New Project', projectPath, projectId);
            setSessionState(state);
          }
        } catch (err) {
          setError(err instanceof Error ? err.message : 'Failed to initialize session');
        } finally {
          setIsLoading(false);
        }
      };

      if (modal.visible) {
        initSession();
      }
    }, [modal.visible, existingSessionId, projectPath, projectId]);

    const handleSendMessage = useCallback(async () => {
      if (!sessionState || !inputValue.trim() || isLoading) return;

      setIsLoading(true);
      setError(null);

      try {
        const response = await gsdApi.sendMessage(sessionState.session.id, inputValue.trim());

        // Update state with new messages
        setSessionState((prev) => {
          if (!prev) return prev;

          const newMessages = [...prev.messages, response.user_message];
          if (response.assistant_response) {
            newMessages.push(...response.assistant_response.messages);
          }

          return {
            ...prev,
            messages: newMessages,
            pending_interaction: response.assistant_response?.pending_interaction || null,
            generated_tasks: response.assistant_response?.generated_tasks || prev.generated_tasks,
          };
        });

        setInputValue('');
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to send message');
      } finally {
        setIsLoading(false);
      }
    }, [sessionState, inputValue, isLoading]);

    const handleResolveInteraction = useCallback(
      async (value: unknown, displayText: string) => {
        if (!sessionState?.pending_interaction || isLoading) return;

        setIsLoading(true);
        setError(null);

        // Create a local user message to display the selection immediately
        const userSelectionMessage: GsdMessage = {
          id: `local-${Date.now()}`,
          session_id: sessionState.session.id,
          role: 'user',
          content: displayText,
          message_type: 'message',
          metadata: '',
          created_at: new Date().toISOString(),
        };

        // Add user selection message immediately for visual feedback
        setSessionState((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            messages: [...prev.messages, userSelectionMessage],
            pending_interaction: null, // Hide interaction while loading
          };
        });

        try {
          const response = await gsdApi.resolveInteraction(
            sessionState.session.id,
            sessionState.pending_interaction.id,
            value
          );

          // Update state with new messages from the server
          setSessionState((prev) => {
            if (!prev) return prev;

            // Keep our user selection message and add server messages
            const newMessages = [...prev.messages, ...response.messages];

            return {
              ...prev,
              messages: newMessages,
              pending_interaction: response.pending_interaction,
              generated_tasks:
                response.generated_tasks.length > 0
                  ? response.generated_tasks
                  : prev.generated_tasks,
            };
          });
        } catch (err) {
          setError(err instanceof Error ? err.message : 'Failed to process response');
          // Restore pending interaction on error
          setSessionState((prev) => {
            if (!prev) return prev;
            return {
              ...prev,
              pending_interaction: sessionState.pending_interaction,
            };
          });
        } finally {
          setIsLoading(false);
        }
      },
      [sessionState, isLoading]
    );

    const handleFinalize = useCallback(async () => {
      if (!sessionState) return;

      setIsLoading(true);
      setError(null);

      try {
        // First approve all tasks
        await gsdApi.approveAllTasks(sessionState.session.id);

        // Then finalize
        const result = await gsdApi.finalizeSession(
          sessionState.session.id,
          sessionState.session.title,
          [] // TODO: Allow selecting repositories
        );

        modal.resolve({
          status: 'completed',
          projectId: result.project_id,
        } as GsdSessionDialogResult);
        modal.hide();
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to finalize session');
      } finally {
        setIsLoading(false);
      }
    }, [sessionState, modal]);

    const handleCancel = useCallback(() => {
      modal.resolve({ status: 'cancelled' } as GsdSessionDialogResult);
      modal.hide();
    }, [modal]);

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && !e.shiftKey && !sessionState?.pending_interaction) {
        e.preventDefault();
        handleSendMessage();
      }
    };

    return (
      <Dialog open={modal.visible} onOpenChange={(open) => !open && handleCancel()}>
        <DialogContent className="sm:max-w-[1000px] lg:max-w-[1300px] xl:max-w-[1500px] h-[90vh] max-h-[900px] flex flex-col">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Sparkles className="h-5 w-5 text-primary" />
              GSD - Project Planning Assistant
            </DialogTitle>
          </DialogHeader>

          {/* Messages Area */}
          <div className="flex-1 overflow-y-auto px-1 py-4">
            {isLoading && !sessionState && (
              <div className="flex items-center justify-center h-full">
                <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            )}

            {sessionState?.messages.map((message) => (
              <MessageBubble key={message.id} message={message} />
            ))}

            {/* Pending Interaction */}
            {sessionState?.pending_interaction && (
              <InteractionRenderer
                interaction={sessionState.pending_interaction}
                onResolve={handleResolveInteraction}
                isLoading={isLoading}
              />
            )}

            {/* Tasks Preview */}
            {sessionState && sessionState.generated_tasks.length > 0 && (
              <TasksPreview tasks={sessionState.generated_tasks} />
            )}

            <div ref={messagesEndRef} />
          </div>

          {/* Error Display */}
          {error && (
            <Alert variant="destructive" className="mx-1">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}

          {/* API Not Configured Warning */}
          {apiStatus && !apiStatus.api_configured && (
            <Alert className="mx-1">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>
                {apiStatus.message}
              </AlertDescription>
            </Alert>
          )}

          {/* Backend Indicator */}
          {apiStatus && apiStatus.api_configured && (
            <div className="text-xs text-muted-foreground text-center">
              Powered by {apiStatus.backend}
            </div>
          )}

          {/* Input Area */}
          <div className="border-t pt-4 space-y-3">
            {/* Show input only when there's no pending interaction */}
            {!sessionState?.pending_interaction && (
              <div className="flex gap-2">
                <Textarea
                  value={inputValue}
                  onChange={(e) => setInputValue(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="Describe your project or answer the question..."
                  className="min-h-[60px] resize-none"
                  disabled={isLoading || !sessionState}
                />
                <Button
                  onClick={handleSendMessage}
                  disabled={!inputValue.trim() || isLoading || !sessionState}
                  className="self-end"
                >
                  {isLoading ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Send className="h-4 w-4" />
                  )}
                </Button>
              </div>
            )}

            {/* Action buttons */}
            <div className="flex justify-between">
              <Button variant="outline" onClick={handleCancel}>
                Cancel
              </Button>

              {sessionState &&
                sessionState.generated_tasks.length > 0 &&
                sessionState.session.status === 'active' && (
                  <Button onClick={handleFinalize} disabled={isLoading}>
                    {isLoading ? (
                      <Loader2 className="h-4 w-4 animate-spin mr-2" />
                    ) : (
                      <Check className="h-4 w-4 mr-2" />
                    )}
                    {isLinkedToExistingProject
                      ? `Create Tasks (${sessionState.generated_tasks.length})`
                      : `Create Project (${sessionState.generated_tasks.length} tasks)`}
                  </Button>
                )}
            </div>
          </div>
        </DialogContent>
      </Dialog>
    );
  }
);

export const GsdSessionDialog = defineModal<
  GsdSessionDialogProps,
  GsdSessionDialogResult
>(GsdSessionDialogImpl);
