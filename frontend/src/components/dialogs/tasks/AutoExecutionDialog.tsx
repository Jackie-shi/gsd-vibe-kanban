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
import RepoBranchSelector from '@/components/tasks/RepoBranchSelector';
import { ExecutorProfileSelector } from '@/components/settings';
import {
  useProjectRepos,
  useRepoBranchSelection,
} from '@/hooks';
import { useProject } from '@/contexts/ProjectContext';
import { useUserSystem } from '@/components/ConfigProvider';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import type { ExecutorProfileId } from 'shared/types';

export interface AutoExecutionDialogProps {
  onStart: (targetBranch: string, executorProfileId: string) => Promise<void>;
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

    const {
      configs: repoBranchConfigs,
      isLoading: isLoadingBranches,
      setRepoBranch,
      reset: resetBranchSelection,
    } = useRepoBranchSelection({
      repos: projectRepos,
      enabled: modal.visible && projectRepos.length > 0,
    });

    useEffect(() => {
      if (!modal.visible) {
        setUserSelectedProfile(null);
        setError(null);
        setIsStarting(false);
        resetBranchSelection();
      }
    }, [modal.visible, resetBranchSelection]);

    const defaultProfile = config?.executor_profile ?? null;
    const effectiveProfile = userSelectedProfile ?? defaultProfile;

    const isLoadingInitial = isLoadingRepos || isLoadingBranches;

    const firstBranch = repoBranchConfigs[0]?.targetBranch;
    const allBranchesSelected = repoBranchConfigs.every(
      (c) => c.targetBranch !== null
    );

    const canStart = Boolean(
      effectiveProfile &&
        allBranchesSelected &&
        projectRepos.length > 0 &&
        !isStarting &&
        !isLoadingInitial
    );

    const handleStart = async () => {
      if (!effectiveProfile || !firstBranch) return;
      setIsStarting(true);
      setError(null);
      try {
        const executorProfileIdStr = JSON.stringify(effectiveProfile);
        await onStart(firstBranch, executorProfileIdStr);
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
                'Automatically execute all tasks across phases sequentially. Tasks will be auto-merged after completion, pausing between phases for review.'
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

            <RepoBranchSelector
              configs={repoBranchConfigs}
              onBranchChange={setRepoBranch}
              isLoading={isLoadingBranches}
              className="space-y-2"
            />

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
