import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ApiSite, sitesQueryKey } from "../hooks/useSites";
import { API } from "../config/api";

export function useUpdateSite() {
  const queryClient = useQueryClient();

  const update = useMutation({
    mutationFn: async (site: ApiSite) => {
      const response = await fetch(API.sites, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(site),
      });
      if (!response.ok) {
        const text = await response.text();
        throw new Error(text || `Failed to save site (${response.status})`);
      }
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sitesQueryKey }),
  });

  const remove = useMutation({
    mutationFn: async (siteName: string) => {
      const response = await fetch(API.siteDelete(siteName), { method: "DELETE" });
      if (!response.ok) {
        const text = await response.text();
        throw new Error(text || `Failed to delete site (${response.status})`);
      }
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sitesQueryKey }),
  });

  const updateSite = async (site: ApiSite): Promise<boolean> => {
    try {
      await update.mutateAsync(site);
      return true;
    } catch (error) {
      console.error("useUpdateSite: Failed to save site:", error);
      return false;
    }
  };

  const deleteSite = async (siteName: string): Promise<boolean> => {
    try {
      await remove.mutateAsync(siteName);
      return true;
    } catch (error) {
      console.error("useUpdateSite: Failed to delete site:", error);
      return false;
    }
  };

  return {
    updateSite,
    deleteSite,
    saving: update.isPending,
    deleting: remove.isPending,
  };
}
