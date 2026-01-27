import { useState, useCallback, useRef, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle, Send, Check, Loader2, Sparkles } from 'lucide-react';
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

export interface GsdSessionDialogProps {
  existingSessionId?: string;
}

export type GsdSessionDialogResult =
  | { status: 'completed'; projectId: string }
  | { status: 'cancelled' };

// ============================================================================
// Subcomponents
// ============================================================================

// Message display component
const MessageBubble = ({ message }: { message: GsdMessage }) => {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  return (
    <div
      className={cn(
        'flex w-full mb-3',
        isUser ? 'justify-end' : 'justify-start'
      )}
    >
      <div
        className={cn(
          'max-w-[85%] rounded-lg px-4 py-2',
          isUser
            ? 'bg-primary text-primary-foreground'
            : isSystem
              ? 'bg-muted/50 text-muted-foreground italic'
              : 'bg-muted'
        )}
      >
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
        ) : (
          <div className="whitespace-pre-wrap">{message.content}</div>
        )}
      </div>
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
  onResolve: (value: unknown) => void;
  isLoading: boolean;
}) => {
  const [textValue, setTextValue] = useState('');
  const [selectedValues, setSelectedValues] = useState<string[]>([]);

  const options: GsdInteractionOption[] = interaction.options
    ? JSON.parse(interaction.options)
    : [];

  const handleSingleChoice = (value: string) => {
    onResolve(value);
  };

  const handleMultiChoice = () => {
    onResolve(selectedValues);
  };

  const handleTextSubmit = () => {
    if (textValue.trim()) {
      onResolve(textValue.trim());
    }
  };

  const handleConfirmation = (confirmed: boolean) => {
    onResolve(confirmed);
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
              onClick={() => handleSingleChoice(option.value)}
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
            onClick={handleTextSubmit}
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
  ({ existingSessionId }) => {
    const modal = useModal();
    const [sessionState, setSessionState] = useState<GsdSessionState | null>(null);
    const [apiStatus, setApiStatus] = useState<GsdStatusResponse | null>(null);
    const [inputValue, setInputValue] = useState('');
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const messagesEndRef = useRef<HTMLDivElement>(null);

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
            const state = await gsdApi.createSession('New Project');
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
    }, [modal.visible, existingSessionId]);

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
      async (value: unknown) => {
        if (!sessionState?.pending_interaction || isLoading) return;

        setIsLoading(true);
        setError(null);

        try {
          const response = await gsdApi.resolveInteraction(
            sessionState.session.id,
            sessionState.pending_interaction.id,
            value
          );

          // Update state with new messages
          setSessionState((prev) => {
            if (!prev) return prev;

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
        <DialogContent className="sm:max-w-[700px] h-[80vh] flex flex-col">
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
                    Create Project ({sessionState.generated_tasks.length} tasks)
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
