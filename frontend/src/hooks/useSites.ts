import { useQuery } from "@tanstack/react-query";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface ApiLocation {
  latitude: number;
  longitude: number;
  name: string;
  country: string | null;
}

export type SiteType = "Hang" | "Winch";

export interface ApiLaunch {
  location: ApiLocation;
  direction_degrees_start: number;
  direction_degrees_stop: number;
  elevation: number;
  site_type: SiteType;
}

export interface ApiLanding {
  location: ApiLocation;
  elevation: number;
}

export interface ApiActivity {
  id: string;
  kind: string;
  title: string;
  latitude: number;
  longitude: number;
  description: string;
  image_urls: string[];
  launches: ApiLaunch[];
  landings: ApiLanding[];
  country: string | null;
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
    queryFn: () => fetchJson<ApiActivity[]>(API.sites),
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