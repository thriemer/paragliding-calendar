import { vi } from "vitest";
import React from "react";

// Stub react-leaflet so tests render in jsdom without touching real maps.
vi.mock("react-leaflet", () => {
  const clickHandlerRefs: Array<(e: { latlng: { lat: number; lng: number } }) => void> = [];
  const stableMap = {
    getCenter: () => ({ lat: 47, lng: 10 }),
    getZoom: () => 6,
  };
  return {
    __clickHandlerRefs: clickHandlerRefs,
    MapContainer: ({ children, ...rest }: { children?: React.ReactNode } & Record<string, unknown>) =>
      React.createElement(
        "div",
        {
          "data-testid": "map-container",
          "data-center": JSON.stringify(rest.center),
          "data-zoom": String(rest.zoom),
        },
        children,
      ),
    TileLayer: () => React.createElement("div", { "data-testid": "tile-layer" }),
    Marker: ({ children, position, draggable, eventHandlers }: { children?: React.ReactNode; position?: [number, number]; draggable?: boolean; eventHandlers?: Record<string, (e: unknown) => void> }) => {
      // Expose dragend handler so tests can fire it.
      const ref = (el: HTMLDivElement | null) => {
        if (el && eventHandlers?.dragend) {
          (el as HTMLDivElement & { __dragend?: (e: unknown) => void }).__dragend = eventHandlers.dragend;
        }
      };
      return React.createElement(
        "div",
        {
          "data-testid": "marker",
          "data-position": JSON.stringify(position),
          "data-draggable": draggable ? "true" : "false",
          ref,
        },
        children,
      );
    },
    Popup: ({ children }: { children?: React.ReactNode }) =>
      React.createElement("div", { "data-testid": "popup" }, children),
    Circle: ({ center, radius }: { center?: [number, number]; radius?: number }) =>
      React.createElement("div", {
        "data-testid": "circle",
        "data-center": JSON.stringify(center),
        "data-radius": String(radius),
      }),
    useMap: () => stableMap,
    useMapEvents: (handlers: Record<string, (e: unknown) => void>) => {
      if (handlers.click) {
        clickHandlerRefs.push(handlers.click as (e: { latlng: { lat: number; lng: number } }) => void);
      }
      return stableMap;
    },
  };
});

vi.mock("leaflet", () => {
  class Icon {
    options: Record<string, unknown>;
    constructor(options: Record<string, unknown> = {}) {
      this.options = options;
    }
  }
  const Default = function () {};
  Default.prototype = {};
  (Icon as unknown as { Default: unknown }).Default = Default;
  (Default as unknown as { mergeOptions: (o: Record<string, unknown>) => void }).mergeOptions = () => {};
  return {
    default: { Icon },
    Icon,
  };
});

vi.mock("leaflet/dist/leaflet.css", () => ({}));

vi.mock("cesium", () => {
  const noop = () => undefined;
  const Color = {
    RED: "red",
    ORANGE: "orange",
    YELLOW: "yellow",
    LIME: "lime",
    BLUE: "blue",
  };
  return {
    Ion: { defaultAccessToken: "" },
    createWorldTerrainAsync: async () => ({}),
    Color,
    Cartesian3: { fromDegrees: (..._args: number[]) => ({}) },
    PolylineColorAppearance: class {
      constructor(_opts?: unknown) {}
    },
    GeometryInstance: class {
      constructor(_opts?: unknown) {}
    },
    PolylineGeometry: class {
      constructor(_opts?: unknown) {}
    },
    Viewer: class {
      constructor(_opts?: unknown) {}
    },
    noop,
  };
});

vi.mock("resium", () => ({
  Viewer: ({ children }: { children?: React.ReactNode }) =>
    React.createElement("div", { "data-testid": "cesium-viewer" }, children),
  Primitive: () => React.createElement("div", { "data-testid": "cesium-primitive" }),
  Globe: () => React.createElement("div", { "data-testid": "cesium-globe" }),
}));
