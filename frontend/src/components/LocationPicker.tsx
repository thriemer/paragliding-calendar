import { useState } from "react";

interface LocationPickerProps {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  elevation: number;
  onChange: (location: { latitude: number; longitude: number; name: string; country: string | null }, elevation: number) => void;
}

export function LocationPicker({ location, elevation, onChange }: LocationPickerProps) {
  const [isPickerOpen, setIsPickerOpen] = useState(false);
  const [pickLat, setPickLat] = useState(location.latitude);
  const [pickLng, setPickLng] = useState(location.longitude);
  const [pickElev, setPickElev] = useState(elevation);

  const handleOpenPicker = () => {
    setPickLat(location.latitude);
    setPickLng(location.longitude);
    setPickElev(elevation);
    setIsPickerOpen(true);
  };

  const handleConfirm = () => {
    onChange(
      { ...location, latitude: pickLat, longitude: pickLng },
      pickElev
    );
    setIsPickerOpen(false);
  };

  if (isPickerOpen) {
    return (
      <div className="location-picker-modal">
        <div className="location-picker-content">
          <h5>Pick Location on Map</h5>
          <p className="hint">Click on the map to set location</p>
          <div className="picker-inputs">
            <label>
              Latitude:
              <input
                type="number"
                step="0.0001"
                value={pickLat}
                onChange={(e) => setPickLat(parseFloat(e.target.value) || 0)}
              />
            </label>
            <label>
              Longitude:
              <input
                type="number"
                step="0.0001"
                value={pickLng}
                onChange={(e) => setPickLng(parseFloat(e.target.value) || 0)}
              />
            </label>
            <label>
              Elevation (m):
              <input
                type="number"
                value={pickElev}
                onChange={(e) => setPickElev(parseFloat(e.target.value) || 0)}
              />
            </label>
          </div>
          <div className="picker-actions">
            <button className="btn btn-small" onClick={handleConfirm}>Confirm</button>
            <button className="btn btn-small btn-cancel" onClick={() => setIsPickerOpen(false)}>Cancel</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="location-display">
      <span className="location-coords">
        {location.latitude.toFixed(4)}, {location.longitude.toFixed(4)}
      </span>
      <span className="location-elevation">{elevation}m</span>
      <button className="btn btn-small" onClick={handleOpenPicker}>Pick on Map</button>
    </div>
  );
}
