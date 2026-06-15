import { useMutation } from "@tanstack/react-query";
import { API } from "../config/api";

export function useCalendarRefresh() {
  const mutation = useMutation({
    mutationFn: async () => {
      const response = await fetch(API.calendarRefresh, { method: "POST" });
      if (!response.ok) {
        const text = await response.text();
        throw new Error(text || `Failed to refresh calendar (${response.status})`);
      }
    },
  });

  const refresh = async (): Promise<boolean> => {
    try {
      await mutation.mutateAsync();
      return true;
    } catch (error) {
      console.error("useCalendarRefresh: Failed to trigger refresh:", error);
      return false;
    }
  };

  return {
    refresh,
    refreshing: mutation.isPending,
    error: mutation.error instanceof Error ? mutation.error.message : null,
  };
}
