import { memo, useCallback, useEffect, useMemo, useRef } from "react";
import { MapContainer, TileLayer, Marker, Popup, Circle, useMap, useMapEvents } from "react-leaflet";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { ApiActivity } from "../hooks/useSites";
import { UserSettings } from "../hooks/useSettings";
import { kindLabel, kindColor } from "../utils/activityKind";
import { Legend } from "./Legend";
import "../utils/leaflet";
import styles from "./SitesMap.module.css";
import blueMarker from "../assets/marker-icon-2x-blue.png";
import greenMarker from "../assets/marker-icon-2x-green.png";
import violetMarker from "../assets/marker-icon-2x-violet.png";
import redMarker from "../assets/marker-icon-2x-red.png";
import orangeMarker from "../assets/marker-icon-2x-orange.png";
import greyMarker from "../assets/marker-icon-2x-grey.png";
import markerShadow from "leaflet/dist/images/marker-shadow.png";

const createColoredIcon = (iconUrl: string) =>
  new L.Icon({
    iconUrl,
    shadowUrl: markerShadow,
    iconSize: [25, 41],
    iconAnchor: [12, 41],
    popupAnchor: [1, -34],
    shadowSize: [41, 41],
  });

const winchIcon = createColoredIcon(blueMarker);
const hangIcon = createColoredIcon(greenMarker);
const bothIcon = createColoredIcon(violetMarker);
const landingIcon = createColoredIcon(redMarker);
const userLocationIcon = createColoredIcon(orangeMarker);
const unknownIcon = createColoredIcon(greyMarker);

const circlePathOptions = {
  color: "#000000",
  weight: 2,
  opacity: 0.7,
  fillColor: "#000000",
  fillOpacity: 0.05,
  dashArray: "5, 5",
};

type MapView = { center: [number, number]; zoom: number };

interface SitesMapProps {
  activities: ApiActivity[];
  onSiteClick?: (activity: ApiActivity) => void;
  mapView: MapView | null;
  onMapViewChange: (view: MapView) => void;
  settings?: UserSettings;
}

function MapController({ onMapViewChange }: { onMapViewChange: (view: MapView) => void }) {
  const map = useMap();
  const cbRef = useRef(onMapViewChange);
  cbRef.current = onMapViewChange;

  const report = useCallback(() => {
    const c = map.getCenter();
    cbRef.current({ center: [c.lat, c.lng], zoom: map.getZoom() });
  }, [map]);

  useEffect(() => {
    report();
  }, [report]);

  const handlers = useMemo(
    () => ({ zoomend: report, moveend: report }),
    [report],
  );
  useMapEvents(handlers);
  return null;
}

type SiteType = "winch" | "hang" | "both" | "none";

function getSiteType(site: ApiActivity): SiteType {
  const types = new Set(site.launches.map((l) => l.site_type));
  const hasWinch = types.has("Winch");
  const hasHang = types.has("Hang");

  if (hasWinch && hasHang) return "both";
  if (hasWinch) return "winch";
  if (hasHang) return "hang";
  return "none";
}

function getParaglidingIcon(type: SiteType): L.Icon {
  if (type === "winch") return winchIcon;
  if (type === "hang") return hangIcon;
  if (type === "both") return bothIcon;
  return unknownIcon;
}

function siteTypeLabel(type: SiteType): string {
  if (type === "both") return "Winch + Hang";
  if (type === "winch") return "Winch";
  if (type === "hang") return "Hang";
  return "Unknown";
}

function coordsMatch(a: { lat: number; lng: number }, b: { lat: number; lng: number }, tolerance = 0.0001): boolean {
  return Math.abs(a.lat - b.lat) < tolerance && Math.abs(a.lng - b.lng) < tolerance;
}

interface LaunchData {
  location: { lat: number; lng: number };
  elevation: number;
  siteName: string;
  siteCountry: string | null;
  siteType: string;
}

interface LandingData {
  location: { lat: number; lng: number };
  elevation: number;
  siteName: string;
  siteCountry: string | null;
}

type LaunchWithOverlap = LaunchData & { hasLandingAtSameLocation: boolean };
type LandingWithOverlap = LandingData & { hasLaunchAtSameLocation: boolean };

function PopupEditButton({ onClick }: { onClick: () => void }) {
  return (
    <button className={styles.popupEditBtn} onClick={onClick}>
      Edit
    </button>
  );
}

const LandingMarker = memo(function LandingMarker({
  landing,
  onEdit,
}: {
  landing: LandingWithOverlap;
  onEdit?: (siteName: string) => void;
}) {
  return (
    <Marker
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
        {landing.hasLaunchAtSameLocation && (
          <>
            <br />
            <em>Note: Launch also nearby</em>
          </>
        )}
        {onEdit && (
          <>
            <br />
            <PopupEditButton onClick={() => onEdit(landing.siteName)} />
          </>
        )}
      </Popup>
    </Marker>
  );
});

const LaunchMarker = memo(function LaunchMarker({
  launch,
  onEdit,
}: {
  launch: LaunchWithOverlap;
  onEdit?: (siteName: string) => void;
}) {
  const icon = launch.siteType === "Winch" ? winchIcon : hangIcon;
  return (
    <Marker
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
        {launch.hasLandingAtSameLocation && (
          <>
            <br />
            <em>Note: Landing also nearby</em>
          </>
        )}
        {onEdit && (
          <>
            <br />
            <PopupEditButton onClick={() => onEdit(launch.siteName)} />
          </>
        )}
      </Popup>
    </Marker>
  );
});

const ParaglidingOverviewMarker = memo(function ParaglidingOverviewMarker({
  activity,
  launch,
  onEdit,
}: {
  activity: ApiActivity;
  launch: ApiActivity["launches"][number];
  onEdit?: (siteName: string) => void;
}) {
  const type = getSiteType(activity);
  return (
    <Marker
      position={[launch.location.latitude, launch.location.longitude]}
      icon={getParaglidingIcon(type)}
    >
      <Popup>
        <strong>{activity.title}</strong>
        <br />
        Type: {siteTypeLabel(type)}
        <br />
        {activity.country}
        <br />
        Elevation: {launch.elevation}m
        {onEdit && (
          <>
            <br />
            <PopupEditButton onClick={() => onEdit(activity.id)} />
          </>
        )}
      </Popup>
    </Marker>
  );
});

const ActivityMarker = memo(function ActivityMarker({
  activity,
  onEdit,
}: {
  activity: ApiActivity;
  onEdit?: (id: string) => void;
}) {
  const color = kindColor(activity.kind);
  const markerMap: Record<string, string> = {
    "#0288d1": blueMarker,
    "#388e3c": greenMarker,
    "#f57c00": orangeMarker,
    "#d32f2f": redMarker,
    "#5d4037": violetMarker,
    "#00838f": greyMarker,
  };
  const icon = createColoredIcon(markerMap[color] ?? greyMarker);
  return (
    <Marker
      position={[activity.latitude, activity.longitude]}
      icon={icon}
    >
      <Popup>
        <strong>{activity.title}</strong>
        <br />
        {kindLabel(activity.kind)}
        {activity.description && (
          <>
            <br />
            {activity.description.slice(0, 100)}
            {activity.description.length > 100 ? "…" : ""}
          </>
        )}
        {onEdit && (
          <>
            <br />
            <PopupEditButton onClick={() => onEdit(activity.id)} />
          </>
        )}
      </Popup>
    </Marker>
  );
});

export function SitesMap({ activities, onSiteClick, mapView, onMapViewChange, settings }: SitesMapProps) {
  const { paraglidingSites, otherActivities, center } = useMemo(() => {
    const paraglidingSites = activities.filter((a) => a.kind === "paragliding");
    const otherActivities = activities.filter((a) => a.kind !== "paragliding");
    const allPositions = activities
      .filter((a) => a.latitude != null && a.longitude != null)
      .map((a) => ({ lat: a.latitude, lng: a.longitude }));
    const center: [number, number] =
      allPositions.length > 0
        ? [
            allPositions.reduce((sum, p) => sum + p.lat, 0) / allPositions.length,
            allPositions.reduce((sum, p) => sum + p.lng, 0) / allPositions.length,
          ]
        : [47.0, 10.0];
    return { paraglidingSites, otherActivities, center };
  }, [activities]);

  const { launchesWithOverlap, landingsWithOverlap } = useMemo(() => {
    const paraglidingSites = activities.filter((a) => a.kind === "paragliding");
    const launches: LaunchData[] = paraglidingSites
      .flatMap((site) =>
        site.launches.map((l) => ({
          location: { lat: l.location.latitude, lng: l.location.longitude },
          elevation: l.elevation,
          siteName: site.title,
          siteCountry: site.country,
          siteType: l.site_type,
        })),
      )
      .filter((loc) => loc.location.lat != null && loc.location.lng != null);
    const landings: LandingData[] = paraglidingSites
      .flatMap((site) =>
        site.landings.map((l) => ({
          location: { lat: l.location.latitude, lng: l.location.longitude },
          elevation: l.elevation,
          siteName: site.title,
          siteCountry: site.country,
        })),
      )
      .filter((loc) => loc.location.lat != null && loc.location.lng != null);
    const launchesWithOverlap: LaunchWithOverlap[] = launches.map((launch) => ({
      ...launch,
      hasLandingAtSameLocation: landings.some((landing) => coordsMatch(launch.location, landing.location)),
    }));
    const landingsWithOverlap: LandingWithOverlap[] = landings.map((landing) => ({
      ...landing,
      hasLaunchAtSameLocation: launches.some((launch) => coordsMatch(launch.location, landing.location)),
    }));
    return { launchesWithOverlap, landingsWithOverlap };
  }, [activities]);

  const isZoomedIn = mapView ? mapView.zoom >= 11 : false;
  const mapCenter = mapView?.center ?? center;
  const hasLocationSettings =
    settings != null && settings.location_latitude != null && settings.location_longitude != null;

  const activitiesRef = useRef(activities);
  activitiesRef.current = activities;
  const onSiteClickRef = useRef(onSiteClick);
  onSiteClickRef.current = onSiteClick;

  const handleEditActivity = useCallback((id: string) => {
    const cb = onSiteClickRef.current;
    if (!cb) return;
    const activity = activitiesRef.current.find((a) => a.id === id);
    if (activity) cb(activity);
  }, []);

  const editHandler = onSiteClick ? handleEditActivity : undefined;

  return (
    <div className={styles.mapContainer}>
      <MapContainer
        center={mapCenter}
        zoom={mapView?.zoom ?? 6}
        preferCanvas
        style={{ height: "100%", width: "100%" }}
      >
        <MapController onMapViewChange={onMapViewChange} />
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
          url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
        />
        {hasLocationSettings && (
          <>
            <Circle
              center={[settings.location_latitude, settings.location_longitude]}
              radius={settings.search_radius_km * 1000}
              pathOptions={circlePathOptions}
            />
            <Marker
              position={[settings.location_latitude, settings.location_longitude]}
              icon={userLocationIcon}
            >
              <Popup>
                <strong>Your Location</strong>
                <br />
                {settings.location_name}
                <br />
                Radius: {settings.search_radius_km} km
              </Popup>
            </Marker>
          </>
        )}
        {isZoomedIn ? (
          <>
            {landingsWithOverlap.map((landing, idx) => (
              <LandingMarker
                key={`landing-${landing.siteName}-${idx}`}
                landing={landing}
                onEdit={editHandler}
              />
            ))}
            {launchesWithOverlap.map((launch, idx) => (
              <LaunchMarker
                key={`launch-${launch.siteName}-${idx}`}
                launch={launch}
                onEdit={editHandler}
              />
            ))}
            {otherActivities.map((activity) => (
              <ActivityMarker
                key={activity.id}
                activity={activity}
                onEdit={editHandler}
              />
            ))}
          </>
        ) : (
          <>
            {paraglidingSites.map((site) => {
              const launch = site.launches[0];
              if (!launch) return null;
              return (
                <ParaglidingOverviewMarker
                  key={site.id}
                  activity={site}
                  launch={launch}
                  onEdit={editHandler}
                />
              );
            })}
            {otherActivities.map((activity) => (
              <ActivityMarker
                key={activity.id}
                activity={activity}
                onEdit={editHandler}
              />
            ))}
          </>
        )}
      </MapContainer>
      <Legend isZoomedIn={isZoomedIn} hasLocationSettings={!!hasLocationSettings} />
    </div>
  );
}