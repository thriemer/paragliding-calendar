// Display helpers for backend ActivityKind strings (kind.as_str() in
// src/domain/activities.rs). Keep the keys in sync with that enum.

export const KIND_LABELS: Record<string, string> = {
  paragliding: "Paragliding",
  hiking: "Hiking",
  biking: "Biking",
  running: "Running",
  mountain_climbing: "Mountain Climbing",
  kayaking: "Kayaking",
  event: "Event",
  commitment: "Commitment",
};

export const KIND_COLORS: Record<string, string> = {
  paragliding: "#0288d1",
  hiking: "#388e3c",
  biking: "#f57c00",
  running: "#d32f2f",
  mountain_climbing: "#5d4037",
  kayaking: "#00838f",
  event: "#7b1fa2",
  commitment: "#455a64",
};

/** Human label for a backend kind string; falls back to the raw value. */
export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/** Badge color for a backend kind string; falls back to a neutral grey. */
export function kindColor(kind: string): string {
  return KIND_COLORS[kind] ?? "#666";
}
