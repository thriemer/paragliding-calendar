import { useState, useEffect } from "react";
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

function withSet(data: SettingsResponse | UserSettings): UserSettings {
  return {
    ...data,
    excluded_calendar_names: new Set(data.excluded_calendar_names),
  };
}

export function useSettings() {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchJson<SettingsResponse>(API.settings)
      .then((data) => setSettings(withSet(data)))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "Failed to load settings"),
      )
      .finally(() => setLoading(false));
  }, []);

  const updateSettings = async (newSettings: UserSettings): Promise<boolean> => {
    setSaving(true);
    setError(null);
    try {
      await fetchJson(API.settings, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...newSettings,
          excluded_calendar_names: [...newSettings.excluded_calendar_names],
        }),
      });
      setSettings(withSet(newSettings));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save settings");
      return false;
    } finally {
      setSaving(false);
    }
  };

  return { settings, loading, saving, error, updateSettings };
}
