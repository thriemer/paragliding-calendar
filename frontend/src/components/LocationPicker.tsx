import { useState } from "react";
import { MapContainer, TileLayer, Marker, useMapEvents } from "react-leaflet";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import styles from "./LocationPicker.module.css";

interface LocationPickerProps {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  elevation: number;
  onChange: (location: { latitude: number; longitude: number; name: string; country: string | null }, elevation: number) => void;
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

export function LocationPicker({ location, elevation, onChange }: LocationPickerProps) {
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
        onChange(
          { ...location, latitude: lat, longitude: lng },
          data.elevation
        );
      }
    } catch (error) {
      console.error("Failed to fetch elevation:", error);
    } finally {
      setLoadingElevation(false);
    }
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

  return (
    <div className={styles.locationPicker}>
      <div className={styles.mapContainer}>
        <MapContainer
          center={[pickLat, pickLng]}
          zoom={13}
          zoomControl={false}
          style={{ height: "150px", width: "180px" }}
        >
          <TileLayer
            attribution=''
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
      <div className={styles.elevation}>
        {loadingElevation ? (
          <span className={styles.elevationLoading}>Loading...</span>
        ) : (
          <span>Elevation: {Math.round(pickElev)}m</span>
        )}
      </div>
    </div>
  );
}
