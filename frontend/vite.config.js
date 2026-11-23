import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    chunkSizeWarningLimit: 600,
    sourcemap: false,
  },
  server: {
    host: "127.0.0.1",
    port: 8001,
    strictPort: true,
  },
  preview: {
    host: "127.0.0.1",
    port: 8001,
  },
});
