import { CompassRose } from "./CompassRose";
import { LocationPicker } from "./LocationPicker";

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
  const handleChange = (field: string, value: any) => {
    const updated = { ...launch };
    if (field === "site_type") {
      updated.site_type = value;
    } else if (field === "elevation") {
      updated.elevation = parseFloat(value) || 0;
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

  return (
    <div className="item-card">
      <div className="item-header">
        <span>Launch {index + 1}</span>
        <button className="btn btn-danger btn-small" onClick={() => onRemove(index)}>Remove</button>
      </div>
      <div className="item-row">
        <label>Type:</label>
        <select
          value={launch.site_type}
          onChange={(e) => handleChange("site_type", e.target.value)}
        >
          <option value="Hang">Hang</option>
          <option value="Winch">Winch</option>
        </select>
      </div>
      <div className="item-row">
        <label>Direction:</label>
        <CompassRose
          startDegrees={launch.direction_degrees_start}
          stopDegrees={launch.direction_degrees_stop}
          onChange={handleDirectionChange}
        />
      </div>
      <div className="item-row">
        <label>Location & Elevation:</label>
        <LocationPicker
          location={launch.location}
          elevation={launch.elevation}
          onChange={handleLocationChange}
        />
      </div>
    </div>
  );
}
