import { useState, useEffect } from "react";

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
}

const API_URL = "/api/sites";

export function useSites() {
  const [sites, setSites] = useState<ApiSite[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    fetch(API_URL)
      .then((res) => res.json())
      .then((data) => {
        setSites(data);
        setLoading(false);
      })
      .catch(console.error);
  }, []);

  const refresh = () => {
    setRefreshing(true);
    fetch(API_URL)
      .then((res) => res.json())
      .then((data) => {
        setSites(data);
        setRefreshing(false);
      })
      .catch(console.error);
  };

  return { sites, loading, refreshing, refresh };
}
