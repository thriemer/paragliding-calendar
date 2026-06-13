import { useState } from "react";
import { CompassRose } from "./CompassRose";
import { LocationPicker } from "./LocationPicker";
import { ApiLaunch, ApiLocation } from "../hooks/useSites";
import styles from "./LaunchEditor.module.css";

interface LaunchEditorProps {
  launch: ApiLaunch;
  index: number;
  onChange: (index: number, launch: ApiLaunch) => void;
  onRemove: (index: number) => void;
}

export function LaunchEditor({ launch, index, onChange, onRemove }: LaunchEditorProps) {
  const [isCollapsed, setIsCollapsed] = useState(true);

  const handleNameChange = (name: string) => {
    onChange(index, { ...launch, location: { ...launch.location, name } });
  };

  const handleSiteTypeChange = (site_type: string) => {
    onChange(index, { ...launch, site_type });
  };

  const handleDirectionChange = (start: number, stop: number) => {
    onChange(index, { ...launch, direction_degrees_start: start, direction_degrees_stop: stop });
  };

  const handleLocationChange = (location: ApiLocation, elevation: number) => {
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
                onChange={(e) => handleNameChange(e.target.value)}
                placeholder="Launch name"
              />
            </div>
          </div>

          <div className={styles.launchRow}>
            <div className={styles.launchField}>
              <label>Type:</label>
              <select
                value={launch.site_type}
                onChange={(e) => handleSiteTypeChange(e.target.value)}
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
