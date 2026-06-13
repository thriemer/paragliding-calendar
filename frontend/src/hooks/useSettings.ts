import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface UserSettings {
  location_name: string;
  location_latitude: number;
  location_longitude: number;
  search_radius_km: number;
  calendar_name: string;
  minimum_flyable_hours: number;
  excluded_calendar_names: Set<string>;
  all_calendar_names: string[];
}

interface SettingsResponse extends Omit<UserSettings, "excluded_calendar_names"> {
  excluded_calendar_names: string[];
}

const settingsQueryKey = ["settings"] as const;

function withSet(data: SettingsResponse): UserSettings {
  return {
    ...data,
    excluded_calendar_names: new Set(data.excluded_calendar_names),
  };
}

function toResponse(settings: UserSettings): SettingsResponse {
  return {
    ...settings,
    excluded_calendar_names: [...settings.excluded_calendar_names],
  };
}

function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

export function useSettings() {
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: settingsQueryKey,
    queryFn: () => fetchJson<SettingsResponse>(API.settings),
    select: withSet,
  });

  const mutation = useMutation({
    mutationFn: async (newSettings: UserSettings) => {
      await fetchJson(API.settings, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(toResponse(newSettings)),
      });
      return newSettings;
    },
    onSuccess: (newSettings) => {
      queryClient.setQueryData<SettingsResponse>(
        settingsQueryKey,
        toResponse(newSettings),
      );
    },
  });

  const error =
    (mutation.error && errorMessage(mutation.error, "Failed to save settings")) ||
    (query.error && errorMessage(query.error, "Failed to load settings")) ||
    null;

  const updateSettings = async (newSettings: UserSettings): Promise<boolean> => {
    try {
      await mutation.mutateAsync(newSettings);
      return true;
    } catch {
      return false;
    }
  };

  return {
    settings: query.data ?? null,
    loading: query.isPending,
    saving: mutation.isPending,
    error,
    updateSettings,
  };
}
