import { defineConfig } from "vite";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import react from "@vitejs/plugin-react";

const here = dirname(fileURLToPath(import.meta.url));
const keyPath = resolve(here, "certs/key.pem");
const certPath = resolve(here, "certs/cert.pem");

// The phone browser refuses navigator.geolocation outside a secure context, so a
// walk on the real ground needs https. The certificate is a local self-signed
// pair made by `make-cert.sh`; without it the server stays on plain http.
const https = existsSync(keyPath) && existsSync(certPath)
  ? { key: readFileSync(keyPath), cert: readFileSync(certPath) }
  : undefined;

const proxy = {
  "/api": {
    target: "http://127.0.0.1:3001",
    changeOrigin: true,
  },
};

export default defineConfig({
  plugins: [react()],
  build: {
    chunkSizeWarningLimit: 600,
    sourcemap: false,
  },
  server: {
    host: true,
    allowedHosts: true,
    port: 8001,
    strictPort: true,
    https,
    proxy,
  },
  preview: {
    host: true,
    allowedHosts: true,
    port: 8001,
    strictPort: true,
    https,
    proxy,
  },
});
