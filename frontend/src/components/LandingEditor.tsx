import { LocationPicker } from "./LocationPicker";

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
  const handleChange = (field: string, value: any) => {
    const updated = { ...landing };
    if (field === "elevation") {
      updated.elevation = parseFloat(value) || 0;
    }
    onChange(index, updated);
  };

  const handleLocationChange = (
    location: { latitude: number; longitude: number; name: string; country: string | null },
    elevation: number
  ) => {
    onChange(index, { ...landing, location, elevation });
  };

  return (
    <div className="item-card">
      <div className="item-header">
        <span>Landing {index + 1}</span>
        <button className="btn btn-danger btn-small" onClick={() => onRemove(index)}>Remove</button>
      </div>
      <div className="item-row">
        <label>Location & Elevation:</label>
        <LocationPicker
          location={landing.location}
          elevation={landing.elevation}
          onChange={handleLocationChange}
        />
      </div>
    </div>
  );
}
