import { useState } from "react";
import { MapContainer, TileLayer, Marker, useMapEvents } from "react-leaflet";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";
import { ApiSite } from "../hooks/useSites";
import { useWeatherModels, WeatherModel } from "../hooks/useWeatherModels";
import { LaunchEditor } from "./LaunchEditor";
import { LandingEditor } from "./LandingEditor";
import styles from "./SiteEditor.module.css";
import { API } from "../config/api";

interface SiteEditorProps {
  site: ApiSite;
  onSave: (updatedSite: ApiSite) => void;
  onDelete?: (siteName: string) => void;
  onCancel: () => void;
}

interface Launch {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  direction_degrees_start: number;
  direction_degrees_stop: number;
  elevation: number;
  site_type: string;
}

interface Landing {
  location: { latitude: number; longitude: number; name: string; country: string | null };
  elevation: number;
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

function StarRating({ rating, onChange }: { rating: number | undefined; onChange: (rating: number) => void }) {
  const [hoverRating, setHoverRating] = useState(0);
  
  return (
    <div className={styles.starRating}>
      {[1, 2, 3, 4, 5].map((star) => (
        <span
          key={star}
          className={`${styles.star} ${star <= (hoverRating || rating || 0) ? styles.starFilled : ""}`}
          onClick={() => onChange(star)}
          onMouseEnter={() => setHoverRating(star)}
          onMouseLeave={() => setHoverRating(0)}
        >
          ★
        </span>
      ))}
    </div>
  );
}

function ParkingLocationPicker({
  location,
  onChange,
  onRemove,
}: {
  location: { latitude: number; longitude: number; name: string; country: string | null } | undefined;
  onChange: (location: { latitude: number; longitude: number; name: string; country: string | null }) => void;
  onRemove: () => void;
}) {
  const defaultLoc = location || { latitude: 47.0, longitude: 10.0, name: "", country: "" };
  const [lat, setLat] = useState(defaultLoc.latitude);
  const [lng, setLng] = useState(defaultLoc.longitude);

  const handleMapClick = (newLat: number, newLng: number) => {
    setLat(newLat);
    setLng(newLng);
    onChange({ ...defaultLoc, latitude: newLat, longitude: newLng });
  };

  return (
    <div className={styles.parkingPicker}>
      <div className={styles.miniMap}>
        <MapContainer
          center={[lat, lng]}
          zoom={13}
          style={{ height: "120px", width: "100%" }}
        >
          <TileLayer
            attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
            url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
          />
          <MapClickHandler onClick={handleMapClick} />
          <Marker position={[lat, lng]} />
        </MapContainer>
      </div>
      <button className="btn btn-small btn-danger" onClick={onRemove}>
        Remove Parking
      </button>
    </div>
  );
}

function RuleEditor({
  rule,
  onChange,
  onRemove,
}: {
  rule: unknown;
  onChange: (rule: unknown) => void;
  onRemove: () => void;
}) {
  const [localRule, setLocalRule] = useState(rule || { nodes: [], edges: [] });

  return (
    <div className={styles.ruleEditor}>
      <JdmConfigProvider>
        <DecisionGraph
          value={localRule}
          onChange={(newRule) => {
            setLocalRule(newRule);
            onChange(newRule);
          }}
        />
      </JdmConfigProvider>
      <button className="btn btn-small btn-danger" onClick={onRemove} style={{ marginTop: "10px" }}>
        Remove Rule
      </button>
    </div>
  );
}

export function SiteEditor({ site, onSave, onDelete, onCancel }: SiteEditorProps) {
  const { models } = useWeatherModels();
  const [name, setName] = useState(site.name);
  const [country, setCountry] = useState(site.country || "");
  const [launches, setLaunches] = useState<Launch[]>(site.launches);
  const [landings, setLandings] = useState<Landing[]>(site.landings);
  const [parkingLocation, setParkingLocation] = useState(site.parking_location);
  const [muteAlerts, setMuteAlerts] = useState(site.mute_alerts || false);
  const [rating, setRating] = useState(site.rating || 0);
  const [preferredWeatherModel, setPreferredWeatherModel] = useState(site.preferred_weather_model);
  const [ruleOverwrite, setRuleOverwrite] = useState(site.rule_overwrite);
  const [showRuleEditor, setShowRuleEditor] = useState(false);

  const addRuleOverwrite = async () => {
    try {
      const response = await fetch(API.decisionGraph);
      if (response.ok) {
        const defaultGraph = await response.json();
        setRuleOverwrite(defaultGraph);
      }
    } catch (error) {
      console.error("Failed to fetch default decision graph:", error);
      setRuleOverwrite({ nodes: [], edges: [] });
    }
  };

  const handleLaunchChange = (index: number, launch: Launch) => {
    const updated = [...launches];
    updated[index] = launch;
    setLaunches(updated);
  };

  const handleLaunchRemove = (index: number) => {
    setLaunches(launches.filter((_, i) => i !== index));
  };

  const handleLandingChange = (index: number, landing: Landing) => {
    const updated = [...landings];
    updated[index] = landing;
    setLandings(updated);
  };

  const handleLandingRemove = (index: number) => {
    setLandings(landings.filter((_, i) => i !== index));
  };

  const addLaunch = () => {
    const defaultLat = launches[0]?.location.latitude || 47.0;
    const defaultLng = launches[0]?.location.longitude || 10.0;
    setLaunches([
      ...launches,
      {
        location: { latitude: defaultLat, longitude: defaultLng, name: "", country: country || "" },
        direction_degrees_start: 0,
        direction_degrees_stop: 360,
        elevation: 0,
        site_type: "Hang",
      },
    ]);
  };

  const addLanding = () => {
    const defaultLat = landings[0]?.location.latitude || 47.0;
    const defaultLng = landings[0]?.location.longitude || 10.0;
    setLandings([
      ...landings,
      {
        location: { latitude: defaultLat, longitude: defaultLng, name: "", country: country || "" },
        elevation: 0,
      },
    ]);
  };

  const handleSave = () => {
    onSave({
      name,
      country: country || null,
      launches,
      landings,
      data_source: site.data_source || "API",
      parking_location: parkingLocation || undefined,
      mute_alerts: muteAlerts || undefined,
      rating: rating > 0 ? rating : undefined,
      preferred_weather_model: preferredWeatherModel || undefined,
      rule_overwrite: ruleOverwrite || undefined,
    });
  };

  const handleDelete = () => {
    if (onDelete && confirm(`Are you sure you want to delete "${site.name}"?`)) {
      onDelete(site.name);
    }
  };

  return (
    <div className={styles.siteEditor}>
      <h3>{site.name ? "Edit Site" : "Create Site"}</h3>
      
      <div className={styles.formGroup}>
        <label>Site Name:</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      
      <div className={styles.formGroup}>
        <label>Country:</label>
        <input
          type="text"
          value={country}
          onChange={(e) => setCountry(e.target.value)}
        />
      </div>

      <div className={styles.formGroup}>
        <label>Rating:</label>
        <StarRating rating={rating} onChange={setRating} />
      </div>

      <div className={styles.formGroup}>
        <label>
          <input
            type="checkbox"
            checked={muteAlerts}
            onChange={(e) => setMuteAlerts(e.target.checked)}
          />
          Mute Site Alerts
        </label>
      </div>

      <div className={styles.formGroup}>
        <label>Preferred Weather Model:</label>
        <select
          value={preferredWeatherModel || ""}
          onChange={(e) => setPreferredWeatherModel(e.target.value || undefined)}
        >
          <option value="">Default</option>
          {models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.name}
            </option>
          ))}
        </select>
      </div>

      <div className={styles.formGroup}>
        <label>Parking Location:</label>
        {parkingLocation ? (
          <ParkingLocationPicker
            location={parkingLocation}
            onChange={setParkingLocation}
            onRemove={() => setParkingLocation(undefined)}
          />
        ) : (
          <button className="btn btn-small" onClick={() => setParkingLocation({
            latitude: launches[0]?.location.latitude || 47.0,
            longitude: launches[0]?.location.longitude || 10.0,
            name: "",
            country: country || null,
          })}>
            Add Parking Location
          </button>
        )}
      </div>

      <div className={styles.formGroup}>
        <label>Rule Overwrite:</label>
        {ruleOverwrite ? (
          <RuleEditor
            rule={ruleOverwrite}
            onChange={setRuleOverwrite}
            onRemove={() => setRuleOverwrite(undefined)}
          />
        ) : (
          <button className="btn btn-small" onClick={addRuleOverwrite}>
            Add Rule Overwrite
          </button>
        )}
      </div>

      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <h4>Launches</h4>
          <button className="btn btn-small" onClick={addLaunch}>+ Add</button>
        </div>
        {launches.map((launch, idx) => (
          <LaunchEditor
            key={idx}
            launch={launch}
            index={idx}
            onChange={handleLaunchChange}
            onRemove={handleLaunchRemove}
          />
        ))}
      </div>

      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <h4>Landings</h4>
          <button className="btn btn-small" onClick={addLanding}>+ Add</button>
        </div>
        {landings.map((landing, idx) => (
          <LandingEditor
            key={idx}
            landing={landing}
            index={idx}
            onChange={handleLandingChange}
            onRemove={handleLandingRemove}
          />
        ))}
      </div>

      <div className={styles.actions}>
        {onDelete && (
          <button className="btn btn-danger" onClick={handleDelete}>
            Delete
          </button>
        )}
        <button className="btn" onClick={handleSave}>Save</button>
        <button className="btn btn-cancel" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}
