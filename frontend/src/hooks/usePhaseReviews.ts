import { useState, useEffect, useCallback } from 'react';
import { tasksApi } from '@/lib/api';
import type { ProjectPhaseReview } from 'shared/types';

export interface UsePhaseReviewsResult {
  reviews: ProjectPhaseReview[];
  reviewedPhases: Set<number>;
  isLoading: boolean;
  completePhaseReview: (phaseNumber: number) => Promise<void>;
}

export const usePhaseReviews = (projectId: string | undefined): UsePhaseReviewsResult => {
  const [reviews, setReviews] = useState<ProjectPhaseReview[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (!projectId) return;
    let cancelled = false;

    const load = async () => {
      try {
        const data = await tasksApi.getPhaseReviews(projectId);
        if (!cancelled) setReviews(data);
      } catch (err) {
        console.error('Failed to load phase reviews:', err);
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    };

    load();
    return () => { cancelled = true; };
  }, [projectId]);

  const reviewedPhases = new Set(reviews.map((r) => r.phase_number));

  const completePhaseReview = useCallback(
    async (phaseNumber: number) => {
      if (!projectId) return;
      const review = await tasksApi.createPhaseReview(projectId, phaseNumber);
      setReviews((prev) => [...prev, review]);
    },
    [projectId]
  );

  return { reviews, reviewedPhases, isLoading, completePhaseReview };
};
