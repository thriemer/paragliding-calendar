import { useState } from "react";
import { LocationPicker } from "./LocationPicker";
import { ApiLanding, ApiLocation } from "../hooks/useSites";
import styles from "./LandingEditor.module.css";

interface LandingEditorProps {
  landing: ApiLanding;
  index: number;
  onChange: (index: number, landing: ApiLanding) => void;
  onRemove: (index: number) => void;
}

export function LandingEditor({ landing, index, onChange, onRemove }: LandingEditorProps) {
  const [isCollapsed, setIsCollapsed] = useState(true);

  const handleNameChange = (name: string) => {
    onChange(index, { ...landing, location: { ...landing.location, name } });
  };

  const handleLocationChange = (location: ApiLocation, elevation: number) => {
    onChange(index, { ...landing, location, elevation });
  };

  const title = landing.location.name || `Landing ${index + 1}`;
  const chevronClass = isCollapsed ? `${styles.chevron} ${styles.chevronCollapsed}` : `${styles.chevron} ${styles.chevronNotCollapsed}`;

  return (
    <div className={styles.landingEditor}>
      <div className={styles.landingHeader} onClick={() => setIsCollapsed(!isCollapsed)}>
        <span className={chevronClass}>▶</span>
        <span className={styles.landingTitle}>
          {title}
        </span>
        <button 
          className="btn btn-danger btn-small" 
          onClick={(e) => {
            e.stopPropagation();
            onRemove(index);
          }}
        >
          Remove
        </button>
      </div>

      {!isCollapsed && (
        <div className={styles.landingContent}>
          <div className={styles.landingRow}>
            <div className={styles.landingField}>
              <label>Name:</label>
              <input
                type="text"
                value={landing.location.name}
                onChange={(e) => handleNameChange(e.target.value)}
                placeholder="Landing name"
              />
            </div>
          </div>

          <div className={styles.landingRow}>
            <div className={styles.landingField}>
              <label>Location:</label>
              <LocationPicker
                location={landing.location}
                elevation={landing.elevation}
                onChange={handleLocationChange}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
