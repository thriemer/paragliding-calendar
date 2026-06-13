import { useState } from "react";
import { MapContainer, TileLayer, Marker } from "react-leaflet";
import type L from "leaflet";
import "leaflet/dist/leaflet.css";
import styles from "./LocationPicker.module.css";
import { API } from "../config/api";
import { ApiLocation } from "../hooks/useSites";
import { MapClickHandler } from "../utils/leaflet";
import { fetchJson } from "../utils/fetchJson";

interface ElevationResponse {
  elevation: number;
}

interface LocationPickerProps {
  location: ApiLocation;
  elevation: number;
  onChange: (location: ApiLocation, elevation: number) => void;
}

export function LocationPicker({ location, elevation, onChange }: LocationPickerProps) {
  const [pickLat, setPickLat] = useState(location.latitude);
  const [pickLng, setPickLng] = useState(location.longitude);
  const [pickElev, setPickElev] = useState(elevation);
  const [loadingElevation, setLoadingElevation] = useState(false);

  const updateLocation = async (lat: number, lng: number) => {
    setPickLat(lat);
    setPickLng(lng);
    const newLocation = { ...location, latitude: lat, longitude: lng };
    onChange(newLocation, pickElev);
    setLoadingElevation(true);
    try {
      const data = await fetchJson<ElevationResponse>(API.elevation(lat, lng));
      setPickElev(data.elevation);
      onChange(newLocation, data.elevation);
    } catch (error) {
      console.error("Failed to fetch elevation:", error);
    } finally {
      setLoadingElevation(false);
    }
  };

  const handleMapClick = (lat: number, lng: number) => {
    updateLocation(lat, lng);
  };

  const handleMarkerDrag = (e: L.LeafletMouseEvent) => {
    const { lat, lng } = e.target.getLatLng();
    updateLocation(lat, lng);
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
