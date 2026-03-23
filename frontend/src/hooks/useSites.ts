import { useState, useEffect } from "react";
import { API } from "../config/api";

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
  rule_overwrite?: unknown;
}

export function useSites() {
  const [sites, setSites] = useState<ApiSite[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    fetch(API.sites)
      .then((res) => res.json())
      .then((data) => {
        setSites(data);
        setLoading(false);
      })
      .catch(console.error);
  }, []);

  const refresh = () => {
    setRefreshing(true);
    fetch(API.sites)
      .then((res) => res.json())
      .then((data) => {
        setSites(data);
        setRefreshing(false);
      })
      .catch(console.error);
  };

  return { sites, loading, refreshing, refresh };
}
