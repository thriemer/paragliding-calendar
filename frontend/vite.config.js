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
    // Public Cesium Ion client token; override via env to rotate without a code change.
    __CESIUM_ION_TOKEN__: JSON.stringify(
      process.env.CESIUM_ION_TOKEN ||
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiIwODQ1NjlhMy01OTZjLTQ5ZTgtYWZjMS05NTdjZTBhYjViMTciLCJpZCI6NDIxMjIxLCJpYXQiOjE3NzY3NjYxMzN9.jY86EZR37l3t4CZKNsjBFYFqqadwYSmQjfZmXpDMlok",
    ),
  },
  build: {
    outDir: "dist",
    sourcemap: true,
    minify: true,
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
