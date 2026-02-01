import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { ExecutorProfileSelector } from '@/components/settings';
import { useProjectRepos } from '@/hooks';
import { useProject } from '@/contexts/ProjectContext';
import { useUserSystem } from '@/components/ConfigProvider';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import type { ExecutorProfileId } from 'shared/types';

export interface AutoExecutionDialogProps {
  onStart: (executorProfileId: string) => Promise<void>;
}

const AutoExecutionDialogImpl = NiceModal.create<AutoExecutionDialogProps>(
  ({ onStart }) => {
    const modal = useModal();
    const { projectId } = useProject();
    const { t } = useTranslation('tasks');
    const { profiles, config } = useUserSystem();
    const [isStarting, setIsStarting] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const [userSelectedProfile, setUserSelectedProfile] =
      useState<ExecutorProfileId | null>(null);

    const { data: projectRepos = [], isLoading: isLoadingRepos } =
      useProjectRepos(projectId, { enabled: modal.visible });

    useEffect(() => {
      if (!modal.visible) {
        setUserSelectedProfile(null);
        setError(null);
        setIsStarting(false);
      }
    }, [modal.visible]);

    const defaultProfile = config?.executor_profile ?? null;
    const effectiveProfile = userSelectedProfile ?? defaultProfile;

    const canStart = Boolean(
      effectiveProfile &&
        projectRepos.length > 0 &&
        !isStarting &&
        !isLoadingRepos
    );

    const handleStart = async () => {
      if (!effectiveProfile) return;
      setIsStarting(true);
      setError(null);
      try {
        const executorProfileIdStr = JSON.stringify(effectiveProfile);
        await onStart(executorProfileIdStr);
        modal.hide();
      } catch (err) {
        console.error('Failed to start auto-execution:', err);
        setError(
          err instanceof Error ? err.message : 'Failed to start auto-execution'
        );
        setIsStarting(false);
      }
    };

    return (
      <Dialog open={modal.visible} onOpenChange={(open) => !open && modal.hide()}>
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle>
              {t('autoExecution.dialogTitle', 'Execute All Phases')}
            </DialogTitle>
            <DialogDescription>
              {t(
                'autoExecution.dialogDescription',
                'Automatically execute all tasks across phases sequentially. Completed tasks will be set to review status for manual merge.'
              )}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {profiles && (
              <div className="space-y-2">
                <ExecutorProfileSelector
                  profiles={profiles}
                  selectedProfile={effectiveProfile}
                  onProfileSelect={setUserSelectedProfile}
                  showLabel={true}
                />
              </div>
            )}

            {error && (
              <div className="text-sm text-destructive">{error}</div>
            )}
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => modal.hide()}
              disabled={isStarting}
            >
              {t('common:buttons.cancel', 'Cancel')}
            </Button>
            <Button onClick={handleStart} disabled={!canStart}>
              {isStarting
                ? t('autoExecution.starting', 'Starting...')
                : t('autoExecution.start', 'Start Execution')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export const AutoExecutionDialog =
  defineModal<AutoExecutionDialogProps, void>(AutoExecutionDialogImpl);
