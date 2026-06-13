import { useQuery } from "@tanstack/react-query";
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
  const query = useQuery({
    queryKey: ["weatherModels"],
    queryFn: () => fetchJson<WeatherModelsResponse>(API.weatherModels),
    staleTime: Infinity,
    select: (data) => data.models ?? [],
  });

  return {
    models: query.data ?? [],
    loading: query.isPending,
    error: query.error
      ? query.error instanceof Error
        ? query.error.message
        : "Failed to load weather models"
      : null,
  };
}
