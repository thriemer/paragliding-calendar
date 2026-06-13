export async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    let detail = "";
    try {
      const errorData = await response.json();
      detail = errorData.message || JSON.stringify(errorData);
    } catch {
      detail = await response.text().catch(() => "");
    }
    throw new Error(
      `${response.status} ${response.statusText}${detail ? `: ${detail}` : ""}`,
    );
  }
  return response.json();
}
