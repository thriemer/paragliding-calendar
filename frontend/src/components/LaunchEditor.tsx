import { useState } from "react";
import { CompassRose } from "./CompassRose";
import { LocationPicker } from "./LocationPicker";
import styles from "./LaunchEditor.module.css";

interface Launch {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  direction_degrees_start: number;
  direction_degrees_stop: number;
  elevation: number;
  site_type: string;
}

interface LaunchEditorProps {
  launch: Launch;
  index: number;
  onChange: (index: number, launch: Launch) => void;
  onRemove: (index: number) => void;
}

export function LaunchEditor({ launch, index, onChange, onRemove }: LaunchEditorProps) {
  const [isCollapsed, setIsCollapsed] = useState(true);

  const handleChange = (field: string, value: any) => {
    const updated = { ...launch };
    if (field === "site_type") {
      updated.site_type = value;
    } else if (field === "name") {
      updated.location = { ...updated.location, name: value };
    } else if (field === "direction_start") {
      updated.direction_degrees_start = parseFloat(value) || 0;
    } else if (field === "direction_stop") {
      updated.direction_degrees_stop = parseFloat(value) || 0;
    }
    onChange(index, updated);
  };

  const handleDirectionChange = (start: number, stop: number) => {
    onChange(index, { ...launch, direction_degrees_start: start, direction_degrees_stop: stop });
  };

  const handleLocationChange = (
    location: { latitude: number; longitude: number; name: string; country: string | null },
    elevation: number
  ) => {
    onChange(index, { ...launch, location, elevation });
  };

  const title = launch.location.name || `Launch ${index + 1}`;
  const chevronClass = isCollapsed ? `${styles.chevron} ${styles.chevronCollapsed}` : `${styles.chevron} ${styles.chevronNotCollapsed}`;

  return (
    <div className={styles.launchEditor}>
      <div className={styles.launchHeader} onClick={() => setIsCollapsed(!isCollapsed)}>
        <span className={chevronClass}>▶</span>
        <span className={styles.launchTitle}>
          {title} ({launch.site_type})
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
        <div className={styles.launchContent}>
          <div className={styles.launchRow}>
            <div className={styles.launchField}>
              <label>Name:</label>
              <input
                type="text"
                value={launch.location.name}
                onChange={(e) => handleChange("name", e.target.value)}
                placeholder="Launch name"
              />
            </div>
          </div>

          <div className={styles.launchRow}>
            <div className={styles.launchField}>
              <label>Type:</label>
              <select
                value={launch.site_type}
                onChange={(e) => handleChange("site_type", e.target.value)}
              >
                <option value="Hang">Hang</option>
                <option value="Winch">Winch</option>
              </select>
            </div>
          </div>
          
          <div className={`${styles.launchRow} ${styles.launchDetails}`}>
            <div className={`${styles.launchField} ${styles.compassField}`}>
              <label>Direction:</label>
              <CompassRose
                startDegrees={launch.direction_degrees_start}
                stopDegrees={launch.direction_degrees_stop}
                onChange={handleDirectionChange}
              />
            </div>
            
            <div className={`${styles.launchField} ${styles.locationField}`}>
              <label>Location:</label>
              <LocationPicker
                location={launch.location}
                elevation={launch.elevation}
                onChange={handleLocationChange}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
