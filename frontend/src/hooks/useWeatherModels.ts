import { useState, useEffect } from "react";
import { API } from "../config/api";

export interface WeatherModel {
  id: string;
  name: string;
}

export function useWeatherModels() {
  const [models, setModels] = useState<WeatherModel[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch(API.weatherModels)
      .then((res) => res.json())
      .then((data) => {
        setModels(data.models || []);
        setLoading(false);
      })
      .catch(console.error);
  }, []);

  return { models, loading };
}
