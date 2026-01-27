import { useEffect, useCallback, useRef, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle } from 'lucide-react';
import { CreateProject, Project } from 'shared/types';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { useProjectMutations } from '@/hooks/useProjectMutations';
import { defineModal } from '@/lib/modals';
import { RepoPickerDialog, RepoPickerResult } from '@/components/dialogs/shared/RepoPickerDialog';
import { GsdSessionDialog } from '@/components/dialogs/gsd/GsdSessionDialog';

export interface ProjectFormDialogProps {}

export type ProjectFormDialogResult =
  | { status: 'saved'; project: Project }
  | { status: 'saved_with_gsd'; project: Project; repoPath: string }
  | { status: 'canceled' };

const ProjectFormDialogImpl = NiceModal.create<ProjectFormDialogProps>(() => {
  const modal = useModal();
  const [pendingGsdInfo, setPendingGsdInfo] = useState<{ repoPath: string } | null>(null);

  const { createProject } = useProjectMutations({
    onCreateSuccess: async (project) => {
      if (pendingGsdInfo) {
        // User selected AI Planning - open GSD dialog
        modal.hide();
        try {
          const gsdResult = await GsdSessionDialog.show({
            projectId: project.id,
            projectPath: pendingGsdInfo.repoPath,
          });
          if (gsdResult.status === 'completed') {
            modal.resolve({ status: 'saved_with_gsd', project, repoPath: pendingGsdInfo.repoPath } as ProjectFormDialogResult);
          } else {
            modal.resolve({ status: 'saved', project } as ProjectFormDialogResult);
          }
        } catch {
          // GSD was cancelled, but project was still created
          modal.resolve({ status: 'saved', project } as ProjectFormDialogResult);
        }
        setPendingGsdInfo(null);
      } else {
        modal.resolve({ status: 'saved', project } as ProjectFormDialogResult);
        modal.hide();
      }
    },
    onCreateError: () => {
      setPendingGsdInfo(null);
    },
  });
  const createProjectMutate = createProject.mutate;

  const hasStartedCreateRef = useRef(false);

  const handlePickRepo = useCallback(async () => {
    const result = await RepoPickerDialog.show({
      title: 'Create Project',
      description: 'Select or create a repository for your project',
      showAiPlanningOption: true,
    });

    if (result) {
      const { repo, useAiPlanning } = result as RepoPickerResult;
      const projectName = repo.display_name || repo.name;

      // Store GSD info if user wants AI Planning
      if (useAiPlanning) {
        setPendingGsdInfo({ repoPath: repo.path });
      }

      const createData: CreateProject = {
        name: projectName,
        repositories: [{ display_name: projectName, git_repo_path: repo.path }],
      };

      createProjectMutate(createData);
    } else {
      modal.resolve({ status: 'canceled' } as ProjectFormDialogResult);
      modal.hide();
    }
  }, [createProjectMutate, modal]);

  useEffect(() => {
    if (!modal.visible) {
      hasStartedCreateRef.current = false;
      return;
    }

    if (hasStartedCreateRef.current) return;
    hasStartedCreateRef.current = true;
    handlePickRepo();
  }, [modal.visible, handlePickRepo]);

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      modal.resolve({ status: 'canceled' } as ProjectFormDialogResult);
      modal.hide();
    }
  };

  return (
    <Dialog
      open={modal.visible && createProject.isPending}
      onOpenChange={handleOpenChange}
    >
      <DialogContent className="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>Creating Project</DialogTitle>
          <DialogDescription>Setting up your project...</DialogDescription>
        </DialogHeader>

        <div className="flex items-center justify-center py-8">
          <div className="animate-spin h-8 w-8 border-4 border-primary border-t-transparent rounded-full" />
        </div>

        {createProject.isError && (
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>
              {createProject.error instanceof Error
                ? createProject.error.message
                : 'Failed to create project'}
            </AlertDescription>
          </Alert>
        )}
      </DialogContent>
    </Dialog>
  );
});

export const ProjectFormDialog = defineModal<
  ProjectFormDialogProps,
  ProjectFormDialogResult
>(ProjectFormDialogImpl);
