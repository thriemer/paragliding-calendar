import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteStaticCopy } from "vite-plugin-static-copy";

export default defineConfig({
  plugins: [
    react(),
    viteStaticCopy({
      targets: [
        {
          src: "node_modules/cesium/Build/Cesium/{ThirdParty,Workers,Assets,Widgets}/**/*",
          dest: "cesium",
          rename: { stripBase: 4 },
        },
      ],
    }),
  ],
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
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});
