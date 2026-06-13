import { useState, useEffect } from "react";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface WeatherModel {
  id: string;
  name: string;
}

interface WeatherModelsResponse {
  models?: WeatherModel[];
}

export function useWeatherModels() {
  const [models, setModels] = useState<WeatherModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchJson<WeatherModelsResponse>(API.weatherModels)
      .then((data) => setModels(data.models || []))
      .catch((err) =>
        setError(err instanceof Error ? err.message : "Failed to load weather models"),
      )
      .finally(() => setLoading(false));
  }, []);

  return { models, loading, error };
}
