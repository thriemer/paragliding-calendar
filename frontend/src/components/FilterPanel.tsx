import { ApiActivity } from "../hooks/useSites";
import { kindLabel, kindColor } from "../utils/activityKind";
import styles from "./FilterPanel.module.css";

export interface Filters {
  activityTypes: string[];
}

interface FilterPanelProps {
  filters: Filters;
  onFilterChange: (filters: Filters) => void;
  activities: ApiActivity[];
}

const EXCLUDED_KINDS = new Set(["event", "commitment"]);

export function FilterPanel({ filters, onFilterChange, activities }: FilterPanelProps) {
  const activityTypes = Array.from(
    new Set(activities.map((a) => a.kind).filter((k) => !EXCLUDED_KINDS.has(k)))
  ).sort();

  const toggleKind = (kind: string) => {
    const active = filters.activityTypes.includes(kind)
      ? filters.activityTypes.filter((k) => k !== kind)
      : [...filters.activityTypes, kind];
    onFilterChange({ ...filters, activityTypes: active });
  };

  return (
    <div className={styles.filterPanel}>
      <div className={styles.filterGroup}>
        <label>Activity Type:</label>
        <div className={styles.checkboxGroup}>
          {activityTypes.map((kind) => (
            <label key={kind} className={styles.checkboxLabel}>
              <input
                type="checkbox"
                checked={filters.activityTypes.length === 0 || filters.activityTypes.includes(kind)}
                onChange={() => toggleKind(kind)}
              />
              <span
                className={styles.kindDot}
                style={{ backgroundColor: kindColor(kind) }}
              />
              {kindLabel(kind)}
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}