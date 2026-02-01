import { useState, useEffect, useCallback, useRef } from 'react';
import { autoExecutionApi } from '@/lib/api';
import type { ProjectAutoExecution, AutoExecutionStatus } from 'shared/types';

export interface UseAutoExecutionResult {
  autoExecution: ProjectAutoExecution | null;
  isLoading: boolean;
  isActive: boolean;
  start: (executorProfileId: string) => Promise<void>;
  cancel: () => Promise<void>;
}

export const useAutoExecution = (
  projectId: string | undefined
): UseAutoExecutionResult => {
  const [autoExecution, setAutoExecution] =
    useState<ProjectAutoExecution | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchStatus = useCallback(async () => {
    if (!projectId) return;
    try {
      const data = await autoExecutionApi.getStatus(projectId);
      setAutoExecution(data);
    } catch (err) {
      console.error('Failed to fetch auto-execution status:', err);
    }
  }, [projectId]);

  // Initial load
  useEffect(() => {
    if (!projectId) {
      setIsLoading(false);
      return;
    }
    let cancelled = false;

    const load = async () => {
      try {
        const data = await autoExecutionApi.getStatus(projectId);
        if (!cancelled) setAutoExecution(data);
      } catch (err) {
        console.error('Failed to load auto-execution status:', err);
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  // Poll when active
  const isActive = autoExecution != null && isActiveStatus(autoExecution.status);

  useEffect(() => {
    if (!isActive || !projectId) {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    intervalRef.current = setInterval(fetchStatus, 3000);
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [isActive, projectId, fetchStatus]);

  const start = useCallback(
    async (executorProfileId: string) => {
      if (!projectId) return;
      const result = await autoExecutionApi.start(projectId, {
        executor_profile_id: executorProfileId,
      });
      setAutoExecution(result);
    },
    [projectId]
  );

  const cancel = useCallback(async () => {
    if (!projectId) return;
    await autoExecutionApi.cancel(projectId);
    setAutoExecution(null);
  }, [projectId]);

  return { autoExecution, isLoading, isActive, start, cancel };
};

function isActiveStatus(status: AutoExecutionStatus): boolean {
  return status === 'running' || status === 'paused_for_review';
}
