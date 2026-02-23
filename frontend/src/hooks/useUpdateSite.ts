import { useState, useEffect } from "react";
import { ApiSite } from "../hooks/useSites";

export function useUpdateSite() {
  const [saving, setSaving] = useState(false);

  const updateSite = async (site: ApiSite) => {
    setSaving(true);
    try {
      const response = await fetch("/api/sites", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(site),
      });
      return response.ok;
    } catch (error) {
      console.error("Failed to save site:", error);
      return false;
    } finally {
      setSaving(false);
    }
  };

  return { updateSite, saving };
}
