import { useState, useEffect } from "react";

export interface WeatherModel {
  id: string;
  name: string;
}

const API_URL = "api/weather-models";

export function useWeatherModels() {
  const [models, setModels] = useState<WeatherModel[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch(API_URL)
      .then((res) => res.json())
      .then((data) => {
        setModels(data.models || []);
        setLoading(false);
      })
      .catch(console.error);
  }, []);

  return { models, loading };
}
