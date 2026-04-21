import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import cesium from "vite-plugin-cesium";

export default defineConfig({
  plugins: [react(), cesium()],
  define: {
    __API_BASE_PATH__: JSON.stringify(process.env.API_BASE_PATH || "/"),
  },
  build: {
    outDir: "dist",
    sourcemap: true,
    minify: true,
  },
  resolve: {
    alias: {
      "@": "/src",
    },
  },
  optimizeDeps: {
    include: ["react", "react-dom"],
  },
  server: {
    port: 3001,
  },
});
