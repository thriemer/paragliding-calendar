import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface PreferenceActivity {
  id: string;
  kind: string;
  title: string;
  description: string;
  /** Free-form label→value pairs shown as key stats (e.g. duration, difficulty). */
  stats: Record<string, string>;
  /** Content hashes of the activity's images (position order, 0 = primary). */
  image_hashes: string[];
  current_score: number;
}

export interface PreferencePairData {
  pair_id: string;
  a: PreferenceActivity;
  b: PreferenceActivity;
}

export interface PreferenceProgressData {
  comparisons_done: number;
}

export interface VoteResponse {
  next: PreferencePairData | null;
  progress: PreferenceProgressData;
}

export interface FeatureSummary {
  feature: string;
  weight: number;
  direction: string;
}

export interface KindSummary {
  base_pref: number;
  activity_count: number;
  features: FeatureSummary[];
}

export interface ValidationMetrics {
  pairwise_accuracy: number | null;
  pairwise_count: number;
  rating_mse: number | null;
  rating_count: number;
  k: number;
}

export interface PreferenceSummary {
  comparisons_done: number;
  ratings_done: number;
  kinds: Record<string, KindSummary>;
  validation: ValidationMetrics;
}

export type ComparisonMatrix = Record<string, Record<string, number>>;

const postJson = <T>(url: string, body: unknown): Promise<T> =>
  fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

export function usePreferences() {
  const queryClient = useQueryClient();
  const [pair, setPair] = useState<PreferencePairData | null>(null);
  const [comparisonsDone, setComparisonsDone] = useState(0);

  const initial = useQuery({
    queryKey: ["preferences", "compare"],
    queryFn: () => fetchJson<PreferencePairData>(API.preferencesCompare),
  });

  const summary = useQuery({
    queryKey: ["preferences", "summary"],
    queryFn: () => fetchJson<PreferenceSummary>(API.preferences),
    refetchInterval: 5_000,
  });

  const matrix = useQuery({
    queryKey: ["preferences", "matrix"],
    queryFn: () => fetchJson<ComparisonMatrix>(API.preferencesMatrix),
    refetchInterval: 5_000,
  });

  const handleVoteResponse = (res: VoteResponse) => {
    setComparisonsDone(res.progress.comparisons_done);
    if (res.next) {
      setPair(res.next);
    } else {
      setPair(null);
    }
    queryClient.invalidateQueries({ queryKey: ["preferences", "summary"] });
    queryClient.invalidateQueries({ queryKey: ["preferences", "matrix"] });
  };

  const voteMutation = useMutation({
    mutationFn: (winnerId: string) =>
      postJson<VoteResponse>(API.preferencesVote, {
        pair_id: currentPair?.pair_id,
        winner_id: winnerId,
      }),
    onSuccess: handleVoteResponse,
  });

  const skipMutation = useMutation({
    mutationFn: () => fetchJson<PreferencePairData>(API.preferencesCompare),
    onSuccess: (p) => setPair(p),
  });

  const rateMutation = useMutation({
    mutationFn: (input: { activityId: string; rating: number }) =>
      postJson<{ ok: boolean }>(API.preferencesRate, {
        activity_id: input.activityId,
        rating: input.rating,
      }),
  });

  const likeBothMutation = useMutation({
    mutationFn: () =>
      postJson<VoteResponse>(API.preferencesLikeBoth, {
        pair_id: currentPair?.pair_id,
      }),
    onSuccess: handleVoteResponse,
  });

  const dislikeBothMutation = useMutation({
    mutationFn: () =>
      postJson<VoteResponse>(API.preferencesDislikeBoth, {
        pair_id: currentPair?.pair_id,
      }),
    onSuccess: handleVoteResponse,
  });

  const reEmbedMutation = useMutation({
    mutationFn: () => fetchJson<{ embedded: number }>(API.preferencesReEmbed, { method: "POST" }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["preferences", "summary"] });
      queryClient.invalidateQueries({ queryKey: ["preferences", "matrix"] });
    },
  });

  const currentPair = pair ?? initial.data ?? null;

  const errorOf = (e: unknown) => (e instanceof Error ? e.message : null);

  return {
    pair: currentPair,
    comparisonsDone,
    loading: initial.isPending,
    voting: voteMutation.isPending || skipMutation.isPending,
    error:
      errorOf(initial.error) ||
      errorOf(voteMutation.error) ||
      errorOf(skipMutation.error),
    vote: (winnerId: string) => voteMutation.mutate(winnerId),
    likeBoth: () => likeBothMutation.mutate(),
    dislikeBoth: () => dislikeBothMutation.mutate(),
    skip: () => skipMutation.mutate(),
    rate: (activityId: string, rating: number) =>
      rateMutation.mutateAsync({ activityId, rating }),
    rating: rateMutation.isPending,
    summary: summary.data ?? null,
    summaryLoading: summary.isPending,
    summaryError: errorOf(summary.error),
    matrix: matrix.data ?? null,
    matrixLoading: matrix.isPending,
    matrixError: errorOf(matrix.error),
    reEmbed: () => reEmbedMutation.mutate(),
    reEmbedding: reEmbedMutation.isPending,
  };
}
