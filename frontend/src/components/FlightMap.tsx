import { Viewer, Entity, PolylineGraphics, Globe } from "resium";
import * as Cesium from "cesium";
import { useEffect, useState, useRef } from "react";
import { TrackPoint } from "../hooks/useFlightAnalytics";

interface FlightMapProps {
  path: TrackPoint[];
}

const ionAccessToken =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiIwODQ1NjlhMy01OTZjLTQ5ZTgtYWZjMS05NTdjZTBhYjViMTciLCJpZCI6NDIxMjIxLCJpYXQiOjE3NzY3NjYxMzN9.jY86EZR37l3t4CZKNsjBFYFqqadwYSmQjfZmXpDMlok";

export function FlightMap({ path }: FlightMapProps) {
  const [terrainProvider, setTerrainProvider] =
    useState<Cesium.TerrainProvider | null>(null);
  const viewerRef = useRef<{ cesiumElement?: Cesium.Viewer } | null>(null);

  // Load terrain (same as before)
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

  // Convert TrackPoint array to Cesium.Cartesian3 array
  const getPathPositions = (): Cesium.Cartesian3[] => {
    return path.map((point) => {
      console.log(point);
      return Cesium.Cartesian3.fromDegrees(
        point.longitude,
        point.latitude,
        point.height,
      );
    });
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
      <Globe depthTestAgainstTerrain={true} enableLighting={false} />
      {path.length > 0 && (
        <Entity name="Flight Path">
          <PolylineGraphics
            positions={getPathPositions()}
            width={4}
            material={Cesium.Color.YELLOW}
            clampToGround={false} // Keep altitude from TrackPoint
            arcType={Cesium.ArcType.GEODESIC}
          />
        </Entity>
      )}
    </Viewer>
  );
}
