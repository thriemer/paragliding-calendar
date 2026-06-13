# TravelAI

An activity planner. It looks at your Google Calendar, finds free windows, and
fills them with suggested activities — scheduling the best ones back as
calendar events.

Activities are pluggable. For now there is one: **paragliding**, which scores
upcoming flyable windows against weather and known DHV sites, and ships with a
React + Cesium UI for managing sites and analysing flight logs (KML).

Stack: Rust (Axum) backend, React + Cesium frontend, fjall on-disk store.

## Run locally

Prerequisites: Rust (edition 2024), Node.js, a Google OAuth client (for the
Calendar integration), and `gpg` if you use the encrypted secrets file.

```bash
# 1. Load secrets into the shell (decrypts .env_enc)
eval "$(./load_env.sh)"

# 2. Build the frontend (served from frontend/dist by the backend)
cd frontend && npm install && npm run build && cd ..

# 3. Run the backend
cargo run --no-default-features --features http   # plain HTTP on :8080
# or, with TLS (requires TLS_CERT_PATH and TLS_KEY_PATH):
cargo run                                         # HTTPS on :8080
```

Required env vars: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`,
`OAUTH_REDIRECT_URL`, `CACHE_DIRECTORY` (or `XDG_CACHE_HOME`).
Optional: `PORT`, `OTEL_EXPORTER_OTLP_ENDPOINT`, `RUST_LOG`.

For frontend-only iteration: `cd frontend && npm run dev` (Vite on :3001).

## Deploy

Deployment is a NixOS module exposed by the flake. On the target host:

```nix
{
  imports = [ inputs.travelai.nixosModules.travelai ];

  services.travelai = {
    enable          = true;
    enableTLS       = true;          # or false for plain HTTP
    port            = 8080;
    redirectUrl     = "https://example.com/oauth/callback";
    secretsFilePath = "/run/secrets/travelai.env";   # EnvironmentFile
    otelEndpoint    = "http://alloy:4318";           # optional
  };
}
```

The module runs the service as a dedicated `travelai` user with a managed
`CacheDirectory`, opens the firewall port, and restarts on failure. Build
artifacts come from `packages.travelai-tls` / `packages.travelai-http` in the
flake.
