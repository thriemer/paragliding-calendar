const BASE_PATH = __API_BASE_PATH__;

const api = (path: string) => `${BASE_PATH}${path}`;

export const API = {
  settings: api("/api/settings"),
  sites: api("/api/sites"),
  siteImport: api("/api/sites/import"),
  siteDelete: (name: string) => api(`/api/sites/${encodeURIComponent(name)}`),
  weatherModels: api("/api/weather-models"),
  decisionGraph: api("/api/decision-graph"),
} as const;
