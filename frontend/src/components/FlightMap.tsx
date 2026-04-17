import { useEffect, useRef } from "react";
import Map, { Source, Layer } from "react-map-gl/maplibre";
import "maplibre-gl/dist/maplibre-gl.css";
import type { LineLayer, RasterDEMSourceSpecification } from "maplibre-gl";
import { TrackPoint } from "../hooks/useFlightAnalytics";

interface FlightMapProps {
  path: TrackPoint[];
}

const flightLineLayer: LineLayer = {
  id: "flight-path",
  type: "line",
  paint: {
    "line-color": "#3b82f6",
    "line-width": 3,
    "line-opacity": 0.8,
  },
};

const mapStyle = {
  version: 8 as const,
  sources: {
    osm: {
      type: "raster" as const,
      tiles: ["https://a.tile.openstreetmap.org/{z}/{x}/{y}.png"],
      tileSize: 256,
      attribution: "&copy; OpenStreetMap Contributors",
      maxzoom: 19,
    },
    terrain: {
      type: "raster-dem" as const,
      url: "https://tiles.mapterhorn.com/tilejson.json",
      tileSize: 256,
    } as RasterDEMSourceSpecification,
  },
  layers: [
    {
      id: "osm",
      type: "raster" as const,
      source: "osm",
    },
    {
      id: "hills",
      type: "hillshade" as const,
      source: "terrain",
      layout: { visibility: "visible" as const },
      paint: { "hillshade-shadow-color": "#473B24" },
    },
  ],
  terrain: {
    source: "terrain",
    exaggeration: 1.5,
  },
  sky: {},
};

export function FlightMap({ path }: FlightMapProps) {
  const mapRef = useRef<any>(null);

  useEffect(() => {
    const map = mapRef.current?.getMap?.();
    if (!map || path.length === 0) return;

    const bounds = path.reduce(
      (bounds, point) => {
        return bounds.extend([point.longitude, point.latitude]);
      },
      new (map as any).constructor.LngLatBounds(
        [path[0].longitude, path[0].latitude],
        [path[0].longitude, path[0].latitude]
      )
    );

    map.fitBounds(bounds, { padding: 50 });
  }, [path]);

  if (path.length === 0) {
    return (
      <div style={{ height: "400px", width: "100%", borderRadius: "8px", overflow: "hidden", background: "#f0f0f0", display: "flex", alignItems: "center", justifyContent: "center" }}>
        <span style={{ color: "#666" }}>No path data available</span>
      </div>
    );
  }

  const geojson: GeoJSON.FeatureCollection = {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        geometry: {
          type: "LineString",
          coordinates: path.map((p) => [p.longitude, p.latitude]),
        },
        properties: {},
      },
    ],
  };

  const initialViewState = {
    longitude: path[0]?.longitude ?? 0,
    latitude: path[0]?.latitude ?? 0,
    zoom: 12,
    pitch: 60,
    bearing: 0,
  };

  return (
    <div style={{ height: "400px", width: "100%", borderRadius: "8px", overflow: "hidden" }}>
      <Map
        ref={mapRef}
        initialViewState={initialViewState}
        style={{ width: "100%", height: "100%" }}
        mapStyle={mapStyle as any}
        attributionControl={false}
        maxPitch={85}
      >
        <Source id="flight-data" type="geojson" data={geojson}>
          <Layer {...flightLineLayer} />
        </Source>
      </Map>
    </div>
  );
}