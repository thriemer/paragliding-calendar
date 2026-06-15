import "cesium/Build/Cesium/Widgets/widgets.css";

declare global {
  interface Window {
    CESIUM_BASE_URL?: string;
  }
}

window.CESIUM_BASE_URL = `${import.meta.env.BASE_URL}cesium/`;
