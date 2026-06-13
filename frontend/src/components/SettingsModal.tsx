import { useState } from "react";
import { MapContainer, TileLayer, Marker } from "react-leaflet";
import "leaflet/dist/leaflet.css";
import styles from "./SettingsModal.module.css";
import { UserSettings } from "../hooks/useSettings";
import { MapClickHandler } from "../utils/leaflet";

interface SettingsModalProps {
  settings: UserSettings;
  onSave: (settings: UserSettings) => void;
  onCancel: () => void;
}

export function SettingsModal({
  settings,
  onSave,
  onCancel,
}: SettingsModalProps) {
  const [locationName, setLocationName] = useState(settings.location_name);
  const [locationLat, setLocationLat] = useState(settings.location_latitude);
  const [locationLng, setLocationLng] = useState(settings.location_longitude);
  const [radius, setRadius] = useState(settings.search_radius_km);
  const [calendarName, setCalendarName] = useState(settings.calendar_name);
  const [minFlyableHours, setMinFlyableHours] = useState(
    settings.minimum_flyable_hours,
  );
  const [excludedCalendarNames, setExcludedCalendarNames] = useState(
    settings.excluded_calendar_names,
  );

  const handleMapClick = (lat: number, lng: number) => {
    setLocationLat(lat);
    setLocationLng(lng);
  };

  const handleSave = () => {
    onSave({
      location_name: locationName,
      location_latitude: locationLat,
      location_longitude: locationLng,
      search_radius_km: radius,
      calendar_name: calendarName,
      minimum_flyable_hours: minFlyableHours,
      excluded_calendar_names: excludedCalendarNames,
      all_calendar_names: settings.all_calendar_names,
    });
  };

  return (
    <div className={styles.modalOverlay}>
      <div className={styles.modal}>
        <h2>Settings</h2>

        <div className={styles.field}>
          <label>Location Name</label>
          <input
            type="text"
            value={locationName}
            onChange={(e) => setLocationName(e.target.value)}
            placeholder="e.g., Gornau/Erz"
          />
        </div>

        <div className={styles.field}>
          <label>Location (click on map to select)</label>
          <div className={styles.mapContainer}>
            <MapContainer
              center={[locationLat, locationLng]}
              zoom={10}
              style={{ height: "250px", width: "100%" }}
            >
              <TileLayer
                attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
                url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
              />
              <MapClickHandler onClick={handleMapClick} />
              <Marker position={[locationLat, locationLng]} />
            </MapContainer>
          </div>
          <span className={styles.coordinates}>
            Lat: {locationLat.toFixed(4)}, Lng: {locationLng.toFixed(4)}
          </span>
        </div>

        <div className={styles.field}>
          <label>Search Radius (km): {radius}</label>
          <input
            type="range"
            min="10"
            max="500"
            value={radius}
            onChange={(e) => setRadius(Number(e.target.value))}
          />
        </div>

        <div className={styles.field}>
          <label>Calendar Name</label>
          <input
            type="text"
            value={calendarName}
            onChange={(e) => setCalendarName(e.target.value)}
            placeholder="e.g., Paragliding"
          />
        </div>

        <div className={styles.field}>
          <label>Minimum Flyable Hours: {minFlyableHours}</label>
          <input
            type="range"
            min="1"
            max="8"
            value={minFlyableHours}
            onChange={(e) => setMinFlyableHours(Number(e.target.value))}
          />
        </div>

        <div className={styles.field}>
          <label>Exclude calendars from free/busy check:</label>
          {settings.all_calendar_names.map((name) => {
            return (
              <label key={name}>
                <input
                  type="checkbox"
                  name="ExcludedCalendars"
                  checked={excludedCalendarNames.has(name)}
                  value={name}
                  onChange={(e) =>
                    setExcludedCalendarNames((excluded) => {
                      const next = new Set(excluded);
                      if (e.target.checked) {
                        next.add(e.target.value);
                      } else {
                        next.delete(e.target.value);
                      }
                      return next;
                    })
                  }
                />
                {name}
              </label>
            );
          })}
        </div>

        <div className={styles.buttons}>
          <button className="btn" onClick={handleSave}>
            Save
          </button>
          <button className="btn btn-secondary" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
