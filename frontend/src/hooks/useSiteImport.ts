import { useMutation, useQueryClient } from "@tanstack/react-query";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";
import { sitesQueryKey } from "./useSites";

export interface ImportResponse {
  imported: number;
}

export function useSiteImport() {
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: (file: File) =>
      fetchJson<ImportResponse>(API.siteImport, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: file,
        signal: AbortSignal.timeout(300000),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sitesQueryKey }),
  });

  const importSites = async (file: File): Promise<ImportResponse | null> => {
    try {
      return await mutation.mutateAsync(file);
    } catch {
      return null;
    }
  };

  return {
    importSites,
    importing: mutation.isPending,
    result: mutation.data ?? null,
    error: mutation.error
      ? mutation.error instanceof Error
        ? mutation.error.message
        : "Upload failed"
      : null,
  };
}
