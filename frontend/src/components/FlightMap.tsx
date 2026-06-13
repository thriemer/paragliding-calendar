import { Viewer, Primitive, Globe } from "resium";
import * as Cesium from "cesium";
import { useEffect, useState, useRef } from "react";
import { TrackPoint } from "../hooks/useFlightAnalytics";

interface FlightMapProps {
  path: TrackPoint[];
}

const ionAccessToken =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiIwODQ1NjlhMy01OTZjLTQ5ZTgtYWZjMS05NTdjZTBhYjViMTciLCJpZCI6NDIxMjIxLCJpYXQiOjE3NzY3NjYxMzN9.jY86EZR37l3t4CZKNsjBFYFqqadwYSmQjfZmXpDMlok";

const getColorForClimbRate = (rate: number): Cesium.Color => {
  if (rate < -2) return Cesium.Color.RED;
  if (rate < -0.5) return Cesium.Color.ORANGE;
  if (rate < 0.5) return Cesium.Color.YELLOW;
  if (rate < 2) return Cesium.Color.LIME;
  return Cesium.Color.BLUE;
};

export function FlightMap({ path }: FlightMapProps) {
  const [terrainProvider, setTerrainProvider] =
    useState<Cesium.TerrainProvider | null>(null);
  const viewerRef = useRef<{ cesiumElement?: Cesium.Viewer } | null>(null);

  useEffect(() => {
    Cesium.Ion.defaultAccessToken = ionAccessToken;
    async function loadTerrain() {
      try {
        const provider = await Cesium.createWorldTerrainAsync({
          requestVertexNormals: true,
          requestWaterMask: false,
        });
        setTerrainProvider(provider);
      } catch (error) {
        console.error("Failed to load terrain:", error);
      }
    }
    loadTerrain();
  }, []);

  const getPathPositions = (): Cesium.Cartesian3[] => {
    return path.map((point) =>
      Cesium.Cartesian3.fromDegrees(
        point.longitude,
        point.latitude,
        point.height,
      ),
    );
  };

  const getPathColors = (): Cesium.Color[] => {
    return path.map((point) => getColorForClimbRate(point.climb_rate));
  };

  if (!terrainProvider) {
    return <div>Loading terrain...</div>;
  }

  return (
    <Viewer
      ref={viewerRef}
      animation={false}
      baseLayerPicker={false}
      fullscreenButton={false}
      vrButton={false}
      geocoder={false}
      homeButton={false}
      infoBox={false}
      sceneModePicker={false}
      selectionIndicator={false}
      timeline={false}
      navigationHelpButton={false}
      navigationInstructionsInitiallyVisible={false}
      projectionPicker={false}
      terrainProvider={terrainProvider}
    >
      <Globe depthTestAgainstTerrain={true} enableLighting={true} />
      {path.length > 1 && (
        <Primitive
          appearance={
            new Cesium.PolylineColorAppearance({
              translucent: false,
            })
          }
          geometryInstances={
            new Cesium.GeometryInstance({
              geometry: new Cesium.PolylineGeometry({
                positions: getPathPositions(),
                colors: getPathColors(),
                colorsPerVertex: true,
                width: 4.0,
              }),
            })
          }
        />
      )}
    </Viewer>
  );
}
