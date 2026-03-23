import { useState, useEffect } from "react";
import { API } from "../config/api";

export interface UserSettings {
  location_name: string;
  location_latitude: number;
  location_longitude: number;
  search_radius_km: number;
  calendar_name: string;
  minimum_flyable_hours: number;
}

export function useSettings() {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetch(API.settings)
      .then((res) => res.json())
      .then((data) => {
        setSettings(data);
        setLoading(false);
      })
      .catch(console.error);
  }, []);

  const updateSettings = async (newSettings: UserSettings): Promise<boolean> => {
    setSaving(true);
    try {
      const response = await fetch(API.settings, {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(newSettings),
      });
      if (response.ok) {
        setSettings(newSettings);
        return true;
      }
      return false;
    } catch (error) {
      console.error("Failed to save settings:", error);
      return false;
    } finally {
      setSaving(false);
    }
  };

  return { settings, loading, saving, updateSettings };
}
