const BASE_PATH = __API_BASE_PATH__;

const api = (path: string) => {
  const base = BASE_PATH.replace(/\/+$/, '');
  const normalizedPath = path.replace(/^\/+/, '');
  return `${base}/${normalizedPath}`;
};

export const API = {
  settings: api("/api/settings"),
  sites: api("/api/sites"),
  siteImport: api("/api/sites/import"),
  siteDelete: (name: string) => api(`/api/sites/${encodeURIComponent(name)}`),
  weatherModels: api("/api/weather-models"),
  decisionGraph: api("/api/decision-graph"),
  elevation: (lat: number, lng: number) => api(`/api/elevation?latitude=${lat}&longitude=${lng}`),
} as const;
