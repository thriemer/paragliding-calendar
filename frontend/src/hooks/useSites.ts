import { useQuery } from "@tanstack/react-query";
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

export const sitesQueryKey = ["sites"] as const;

export function useSites() {
  const query = useQuery({
    queryKey: sitesQueryKey,
    queryFn: () => fetchJson<ApiSite[]>(API.sites),
  });

  return {
    sites: query.data ?? [],
    loading: query.isPending,
    error: query.error
      ? query.error instanceof Error
        ? query.error.message
        : "Failed to load sites"
      : null,
  };
}
