import { useState } from "react";
import { ApiSite } from "../hooks/useSites";
import { API } from "../config/api";

export function useUpdateSite() {
  const [saving, setSaving] = useState(false);

  const updateSite = async (site: ApiSite) => {
    setSaving(true);
    try {
      const response = await fetch(API.sites, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(site),
      });
      if (!response.ok) {
        const text = await response.text();
        console.error("useUpdateSite: Failed to save site:", text);
      }
      return response.ok;
    } catch (error) {
      console.error("useUpdateSite: Failed to save site:", error);
      return false;
    } finally {
      setSaving(false);
    }
  };

  return { updateSite, saving };
}

export async function deleteSite(siteName: string): Promise<boolean> {
  try {
    const response = await fetch(API.siteDelete(siteName), {
      method: "DELETE",
    });
    if (!response.ok) {
      const text = await response.text();
      console.error("useUpdateSite: Failed to delete site:", text);
    }
    return response.ok;
  } catch (error) {
    console.error("useUpdateSite: Failed to delete site:", error);
    return false;
  }
}
