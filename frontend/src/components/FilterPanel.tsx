import { ApiSite } from "../hooks/useSites";
import styles from "./FilterPanel.module.css";

export interface Filters {
  siteType: string;
}

interface FilterPanelProps {
  filters: Filters;
  onFilterChange: (filters: Filters) => void;
  sites: ApiSite[];
}

export function FilterPanel({ filters, onFilterChange, sites }: FilterPanelProps) {
  const siteTypes = Array.from(
    new Set(
      sites.flatMap((site) =>
        site.launches.map((launch) => launch.site_type).filter(Boolean)
      )
    )
  ).sort();

  return (
    <div className={styles.filterPanel}>
      <div className={styles.filterGroup}>
        <label>Site Type:</label>
        <select
          value={filters.siteType}
          onChange={(e) => onFilterChange({ ...filters, siteType: e.target.value })}
        >
          <option value="">All</option>
          {siteTypes.map((type) => (
            <option key={type} value={type}>
              {type}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
