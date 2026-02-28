import { useState } from "react";

const API_URL = "/api/sites/import";

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
      const response = await fetch(API_URL, {
        method: "POST",
        headers: {
          "Content-Type": "application/octet-stream",
        },
        body: file,
        signal: AbortSignal.timeout(300000), // 5 minute timeout
      });

      if (!response.ok) {
        let detail = "";
        try {
          const errorData = await response.json();
          detail = errorData.message || JSON.stringify(errorData);
        } catch {
          // Response body might not be JSON
        }
        throw new Error(`Upload failed: ${response.status} ${response.statusText} ${detail}`);
      }

      const data: ImportResponse = await response.json();
      setResult(data);
      return data;
    } catch (err) {
      const message = err instanceof Error ? err.message : "Upload failed";
      setError(message);
      return null;
    } finally {
      setImporting(false);
    }
  };

  return { importSites, importing, result, error };
}
