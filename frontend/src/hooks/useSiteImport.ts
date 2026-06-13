import { useState } from "react";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface ImportResponse {
  imported: number;
}

export function useSiteImport() {
  const [importing, setImporting] = useState(false);
  const [result, setResult] = useState<ImportResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const importSites = async (file: File): Promise<ImportResponse | null> => {
    setImporting(true);
    setError(null);
    setResult(null);

    try {
      const data = await fetchJson<ImportResponse>(API.siteImport, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: file,
        signal: AbortSignal.timeout(300000),
      });
      setResult(data);
      return data;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Upload failed");
      return null;
    } finally {
      setImporting(false);
    }
  };

  return { importSites, importing, result, error };
}
