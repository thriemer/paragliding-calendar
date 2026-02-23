import { useState } from "react";
import { LocationPicker } from "./LocationPicker";
import styles from "./LandingEditor.module.css";

interface Landing {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  elevation: number;
}

interface LandingEditorProps {
  landing: Landing;
  index: number;
  onChange: (index: number, landing: Landing) => void;
  onRemove: (index: number) => void;
}

export function LandingEditor({ landing, index, onChange, onRemove }: LandingEditorProps) {
  const [isCollapsed, setIsCollapsed] = useState(true);

  const handleChange = (field: string, value: any) => {
    const updated = { ...landing };
    if (field === "name") {
      updated.location = { ...updated.location, name: value };
    }
    onChange(index, updated);
  };

  const handleLocationChange = (
    location: { latitude: number; longitude: number; name: string; country: string | null },
    elevation: number
  ) => {
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
                onChange={(e) => handleChange("name", e.target.value)}
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
