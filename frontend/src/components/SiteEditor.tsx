import { useState } from "react";
import { ApiSite } from "../hooks/useSites";
import { LaunchEditor } from "./LaunchEditor";
import { LandingEditor } from "./LandingEditor";

interface SiteEditorProps {
  site: ApiSite;
  onSave: (updatedSite: ApiSite) => void;
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

export function SiteEditor({ site, onSave, onCancel }: SiteEditorProps) {
  const [name, setName] = useState(site.name);
  const [country, setCountry] = useState(site.country || "");
  const [launches, setLaunches] = useState<Launch[]>(site.launches);
  const [landings, setLandings] = useState<Landing[]>(site.landings);

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
        location: { latitude: defaultLat, longitude: defaultLng, name: "", country: site.country },
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
        location: { latitude: defaultLat, longitude: defaultLng, name: "", country: site.country },
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
    });
  };

  return (
    <div className="site-editor">
      <h3>Edit Site</h3>
      
      <div className="form-group">
        <label>Site Name:</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      
      <div className="form-group">
        <label>Country:</label>
        <input
          type="text"
          value={country}
          onChange={(e) => setCountry(e.target.value)}
        />
      </div>

      <div className="section">
        <div className="section-header">
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

      <div className="section">
        <div className="section-header">
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

      <div className="actions">
        <button className="btn" onClick={handleSave}>Save</button>
        <button className="btn btn-cancel" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}
