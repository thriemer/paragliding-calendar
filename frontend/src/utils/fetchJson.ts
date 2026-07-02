export async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    // Read the body once; a second read after a failed .json() would throw.
    const text = await response.text().catch(() => "");
    let detail = text;
    try {
      const errorData = JSON.parse(text);
      detail = errorData.message || JSON.stringify(errorData);
    } catch {
      // not JSON — keep raw text
    }
    throw new Error(
      `${response.status} ${response.statusText}${detail ? `: ${detail}` : ""}`,
    );
  }
  return response.json();
}
