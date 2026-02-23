import { useState } from "react";
import { MapContainer, TileLayer, Marker, useMapEvents } from "react-leaflet";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import styles from "./LocationPicker.module.css";
import launchStyles from "./LaunchEditor.module.css";

interface LocationPickerProps {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  elevation: number;
  onChange: (location: { latitude: number; longitude: number; name: string; country: string | null }, elevation: number) => void;
  inline?: boolean;
}

function fixLeafletIcon() {
  // @ts-ignore
  delete L.Icon.Default.prototype._getIconUrl;
  L.Icon.Default.mergeOptions({
    iconRetinaUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon-2x.png",
    iconUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon.png",
    shadowUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-shadow.png",
  });
}

fixLeafletIcon();

function MapClickHandler({ onClick }: { onClick: (lat: number, lng: number) => void }) {
  useMapEvents({
    click: (e) => {
      onClick(e.latlng.lat, e.latlng.lng);
    },
  });
  return null;
}

export function LocationPicker({ location, elevation, onChange, inline = false }: LocationPickerProps) {
  const [isPickerOpen, setIsPickerOpen] = useState(false);
  const [pickLat, setPickLat] = useState(location.latitude);
  const [pickLng, setPickLng] = useState(location.longitude);
  const [pickElev, setPickElev] = useState(elevation);
  const [loadingElevation, setLoadingElevation] = useState(false);

  const fetchElevation = async (lat: number, lng: number) => {
    setLoadingElevation(true);
    try {
      const response = await fetch(`/api/elevation?latitude=${lat}&longitude=${lng}`);
      if (response.ok) {
        const data = await response.json();
        setPickElev(data.elevation);
        if (inline) {
          onChange(
            { ...location, latitude: lat, longitude: lng },
            data.elevation
          );
        }
      }
    } catch (error) {
      console.error("Failed to fetch elevation:", error);
    } finally {
      setLoadingElevation(false);
    }
  };

  const handleOpenPicker = () => {
    setPickLat(location.latitude);
    setPickLng(location.longitude);
    setPickElev(elevation);
    setIsPickerOpen(true);
    fetchElevation(location.latitude, location.longitude);
  };

  const handleMapClick = (lat: number, lng: number) => {
    setPickLat(lat);
    setPickLng(lng);
    fetchElevation(lat, lng);
  };

  const handleMarkerDrag = (e: L.LeafletMouseEvent) => {
    const { lat, lng } = e.target.getLatLng();
    setPickLat(lat);
    setPickLng(lng);
    fetchElevation(lat, lng);
  };

  const handleConfirm = () => {
    onChange(
      { ...location, latitude: pickLat, longitude: pickLng },
      pickElev
    );
    setIsPickerOpen(false);
  };

  if (inline) {
    return (
      <div className={launchStyles.locationInline}>
        <div className={launchStyles.inlineMapContainer}>
          <MapContainer
            center={[pickLat, pickLng]}
            zoom={13}
            style={{ height: "150px", width: "180px" }}
          >
            <TileLayer
              attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
              url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
            />
            <MapClickHandler onClick={handleMapClick} />
            <Marker
              position={[pickLat, pickLng]}
              draggable={true}
              eventHandlers={{
                dragend: handleMarkerDrag,
              }}
            />
          </MapContainer>
        </div>
        <div className={launchStyles.inlineInfo}>
          <span className={launchStyles.inlineCoords}>{pickLat.toFixed(4)}, {pickLng.toFixed(4)}</span>
          <label>
            <span>Elev (m):</span>
            {loadingElevation ? (
              <span className={launchStyles.elevationLoading}>...</span>
            ) : (
              <input
                type="number"
                value={pickElev}
                onChange={(e) => setPickElev(parseFloat(e.target.value) || 0)}
              />
            )}
          </label>
        </div>
      </div>
    );
  }

  if (isPickerOpen) {
    return (
      <div className={styles.locationPickerModal}>
        <div className={styles.locationPickerContent}>
          <h5>Pick Location on Map</h5>
          <p className={styles.hint}>Click on the map or drag the marker</p>
          <div className={styles.pickerMap}>
            <MapContainer
              center={[pickLat, pickLng]}
              zoom={13}
              style={{ height: "300px", width: "100%" }}
            >
              <TileLayer
                attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
                url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
              />
              <MapClickHandler onClick={handleMapClick} />
              <Marker
                position={[pickLat, pickLng]}
                draggable={true}
                eventHandlers={{
                  dragend: handleMarkerDrag,
                }}
              />
            </MapContainer>
          </div>
          <div className={styles.pickerInputs}>
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
              {loadingElevation ? (
                <span className={launchStyles.elevationLoading}>Loading...</span>
              ) : (
                <input
                  type="number"
                  value={pickElev}
                  onChange={(e) => setPickElev(parseFloat(e.target.value) || 0)}
                />
              )}
            </label>
          </div>
          <div className={styles.pickerActions}>
            <button className="btn btn-small" onClick={handleConfirm}>Confirm</button>
            <button className="btn btn-small btn-cancel" onClick={() => setIsPickerOpen(false)}>Cancel</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={launchStyles.locationDisplay}>
      <span className={launchStyles.locationCoords}>
        {location.latitude.toFixed(4)}, {location.longitude.toFixed(4)}
      </span>
      <span className={launchStyles.locationElevation}>{elevation}m</span>
      <button className="btn btn-small" onClick={handleOpenPicker}>Pick on Map</button>
    </div>
  );
}
