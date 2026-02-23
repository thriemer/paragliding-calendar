import { useState, useEffect } from "react";
import { MapContainer, TileLayer, Marker, Popup, useMap, useMapEvents } from "react-leaflet";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { ApiSite } from "../hooks/useSites";
import { Legend } from "./Legend";

const fixLeafletIcon = () => {
  // @ts-ignore
  delete L.Icon.Default.prototype._getIconUrl;
  L.Icon.Default.mergeOptions({
    iconRetinaUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon-2x.png",
    iconUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon.png",
    shadowUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-shadow.png",
  });
};

fixLeafletIcon();

const createColoredIcon = (color: string) =>
  new L.Icon({
    iconUrl: `https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-2x-${color}.png`,
    shadowUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-shadow.png",
    iconSize: [25, 41],
    iconAnchor: [12, 41],
    popupAnchor: [1, -34],
    shadowSize: [41, 41],
  });

const winchIcon = createColoredIcon("blue");
const hangIcon = createColoredIcon("green");
const bothIcon = createColoredIcon("violet");
const landingIcon = createColoredIcon("red");

interface SitesMapProps {
  sites: ApiSite[];
  onSiteClick?: (site: ApiSite) => void;
}

function MapController({ onZoomChange }: { onZoomChange: (zoom: number) => void }) {
  const map = useMap();

  useEffect(() => {
    onZoomChange(map.getZoom());
  }, []);

  useMapEvents({
    zoomend: () => {
      onZoomChange(map.getZoom());
    },
  });
  return null;
}

function getSiteType(site: ApiSite): "winch" | "hang" | "both" | "none" {
  const types = new Set(site.launches.map((l) => l.site_type));
  const hasWinch = types.has("Winch");
  const hasHang = types.has("Hang");

  if (hasWinch && hasHang) return "both";
  if (hasWinch) return "winch";
  if (hasHang) return "hang";
  return "none";
}

function getMarkerIcon(site: ApiSite): L.Icon {
  const type = getSiteType(site);
  if (type === "winch") return winchIcon;
  if (type === "hang") return hangIcon;
  if (type === "both") return bothIcon;
  return createColoredIcon("grey");
}

function coordsMatch(a: { lat: number; lng: number }, b: { lat: number; lng: number }, tolerance = 0.0001): boolean {
  return Math.abs(a.lat - b.lat) < tolerance && Math.abs(a.lng - b.lng) < tolerance;
}

interface LaunchData {
  location: { latitude: number; longitude: number };
  elevation: number;
  siteName: string;
  siteCountry: string | null;
  siteType: string;
}

interface LandingData {
  location: { latitude: number; longitude: number };
  elevation: number;
  siteName: string;
  siteCountry: string | null;
}

export function SitesMap({ sites, onSiteClick }: SitesMapProps) {
  const [zoom, setZoom] = useState(6);

  const launches: LaunchData[] = sites
    .flatMap((site) =>
      site.launches.map((l) => ({
        location: { lat: l.location.latitude, lng: l.location.longitude },
        elevation: l.elevation,
        siteName: site.name,
        siteCountry: site.country,
        siteType: l.site_type,
      }))
    )
    .filter((loc) => loc.location.lat && loc.location.lng);

  const landings: LandingData[] = sites
    .flatMap((site) =>
      site.landings.map((l) => ({
        location: { lat: l.location.latitude, lng: l.location.longitude },
        elevation: l.elevation,
        siteName: site.name,
        siteCountry: site.country,
      }))
    )
    .filter((loc) => loc.location.lat && loc.location.lng);

  const launchesWithOverlap = launches.map((launch) => {
    const matchingLandings = landings.filter((landing) =>
      coordsMatch(launch.location, landing.location)
    );
    return { ...launch, hasLandingAtSameLocation: matchingLandings.length > 0 };
  });

  const landingsWithOverlap = landings.map((landing) => {
    const matchingLaunches = launches.filter((launch) =>
      coordsMatch(launch.location, landing.location)
    );
    return { ...landing, hasLaunchAtSameLocation: matchingLaunches.length > 0 };
  });

  const allPositions = [...launches, ...landings].map((l) => l.location);

  const center: [number, number] =
    allPositions.length > 0
      ? [
          allPositions.reduce((sum, p) => sum + p.lat, 0) / allPositions.length,
          allPositions.reduce((sum, p) => sum + p.lng, 0) / allPositions.length,
        ]
      : [47.0, 10.0];

  const isZoomedIn = zoom >= 11;

  return (
    <div style={{ position: "relative", height: "100%", width: "100%" }}>
      <MapContainer center={center} zoom={6} style={{ height: "100%", width: "100%" }}>
        <MapController onZoomChange={setZoom} />
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
          url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
        />
        {isZoomedIn ? (
          <>
            {landingsWithOverlap.map((landing, idx) => (
              <Marker
                key={`landing-${landing.siteName}-${idx}`}
                position={[landing.location.lat, landing.location.lng]}
                icon={landingIcon}
                opacity={landing.hasLaunchAtSameLocation ? 0.5 : 1}
              >
                <Popup>
                  <strong>Landing: {landing.siteName}</strong>
                  <br />
                  {landing.siteCountry}
                  <br />
                  Elevation: {landing.elevation}m
                  {landing.hasLaunchAtSameLocation && <><br /><em>Note: Launch also nearby</em></>}
                  {onSiteClick && (
                    <>
                      <br />
                      <button
                        className="popup-edit-btn"
                        onClick={() => {
                          const site = sites.find((s) => s.name === landing.siteName);
                          if (site) onSiteClick(site);
                        }}
                      >
                        Edit
                      </button>
                    </>
                  )}
                </Popup>
              </Marker>
            ))}
            {launchesWithOverlap.map((launch, idx) => {
              const icon = launch.siteType === "Winch" ? winchIcon : hangIcon;
              return (
                <Marker
                  key={`launch-${launch.siteName}-${idx}`}
                  position={[launch.location.lat, launch.location.lng]}
                  icon={icon}
                  opacity={launch.hasLandingAtSameLocation ? 0.5 : 1}
                >
                  <Popup>
                    <strong>Launch: {launch.siteName}</strong>
                    <br />
                    Type: {launch.siteType}
                    <br />
                    {launch.siteCountry}
                    <br />
                    Elevation: {launch.elevation}m
                    {launch.hasLandingAtSameLocation && <><br /><em>Note: Landing also nearby</em></>}
                    {onSiteClick && (
                      <>
                        <br />
                        <button
                          className="popup-edit-btn"
                          onClick={() => {
                            const site = sites.find((s) => s.name === launch.siteName);
                            if (site) onSiteClick(site);
                          }}
                        >
                          Edit
                        </button>
                      </>
                    )}
                  </Popup>
                </Marker>
              );
            })}
          </>
        ) : (
          sites.map((site) =>
            site.launches.map((launch, idx) => (
              <Marker
                key={`${site.name}-${idx}`}
                position={[launch.location.latitude, launch.location.longitude]}
                icon={getMarkerIcon(site)}
              >
                <Popup>
                  <strong>{site.name}</strong>
                  <br />
                  Type: {getSiteType(site) === "both" ? "Winch + Hang" : getSiteType(site) === "winch" ? "Winch" : getSiteType(site) === "hang" ? "Hang" : "Unknown"}
                  <br />
                  {site.country}
                  <br />
                  Elevation: {launch.elevation}m
                  {onSiteClick && (
                    <>
                      <br />
                      <button
                        className="popup-edit-btn"
                        onClick={() => onSiteClick(site)}
                      >
                        Edit
                      </button>
                    </>
                  )}
                </Popup>
              </Marker>
            ))
          )
        )}
      </MapContainer>
      <Legend isZoomedIn={isZoomedIn} />
    </div>
  );
}
