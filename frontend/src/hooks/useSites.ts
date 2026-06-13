import { useState, useEffect } from "react";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface ApiLocation {
  latitude: number;
  longitude: number;
  name: string;
  country: string | null;
}

export interface ApiLaunch {
  location: ApiLocation;
  direction_degrees_start: number;
  direction_degrees_stop: number;
  elevation: number;
  site_type: string;
}

export interface ApiLanding {
  location: ApiLocation;
  elevation: number;
}

export interface ApiSite {
  name: string;
  country: string | null;
  launches: ApiLaunch[];
  landings: ApiLanding[];
  data_source: string;
  parking_location?: ApiLocation;
  mute_alerts?: boolean;
  rating?: number;
  preferred_weather_model?: string;
}

export function useSites() {
  const [sites, setSites] = useState<ApiSite[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async (setBusy: (b: boolean) => void) => {
    setBusy(true);
    setError(null);
    try {
      setSites(await fetchJson<ApiSite[]>(API.sites));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load sites");
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    load(setLoading);
  }, []);

  const refresh = () => load(setRefreshing);

  return { sites, loading, refreshing, error, refresh };
}
