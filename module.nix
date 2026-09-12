{self}: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.travelai;
in {
  options.services.travelai = {
    enable = lib.mkEnableOption "travelai - Intelligent paragliding and outdoor adventure travel planning";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The travelai package to use.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "Port to listen on.";
    };

    enableTLS = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable TLS support. Requires tlsCertPath and tlsKeyPath.";
    };
    logLevel = lib.mkOption {
      type = lib.types.str;
      default = "info";
      description = "Log level that the program should use";
    };
    redirectUrl = lib.mkOption {
      type = lib.types.str;
      description = "Redirect URL for Google OAuth";
    };

    secretsFilePath = lib.mkOption {
      type = lib.types.path;
      description = "Path to the secrets of paragliding calendar.";
    };
    basePath = lib.mkOption {
      type = lib.types.str;
      default = "/";
      description = "Base URL path where the app is mounted (used to build frontend assets).";
    };

    otelEndpoint = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "OpenTelemetry collector endpoint (e.g., http://alloy:4318). Leave empty to disable OTLP export.";
    };

    managePostgres = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Configure a local PostgreSQL instance (with the PostGIS extension) for travelai and wire DATABASE_URL to it automatically via peer auth over the Unix socket. Leave disabled to point DATABASE_URL at an existing/remote database via secretsFilePath instead.";
    };

    databaseName = lib.mkOption {
      type = lib.types.str;
      default = "travelai";
      description = "Name of the PostgreSQL database to use. Only created automatically when managePostgres is enabled.";
    };
  };

  config = lib.mkIf cfg.enable (let
    travelai = pkgs.callPackage ./package.nix {
      enableTLS = cfg.enableTLS;
      basePath = cfg.basePath;
    };
  in
    lib.mkMerge [
      {
        services.travelai.package = travelai;

        systemd.services.travelai = {
          description = "TravelAI - Paragliding and outdoor adventure planning";
          wantedBy = ["multi-user.target"];
          after = ["network.target"] ++ lib.optionals cfg.managePostgres ["postgresql.service"];

          serviceConfig = {
            Type = "simple";
            User = "travelai";
            Group = "travelai";
            WorkingDirectory = "${cfg.package}/bin";
            # Writable, persistent dir (/var/lib/travelai) for the downloaded
            # embedding model — WorkingDirectory is the read-only Nix store.
            StateDirectory = "travelai";
            EnvironmentFile = "${cfg.secretsFilePath}";
            Environment = [
              "PORT=${toString cfg.port}"
              "RUST_LOG=${cfg.logLevel}"
              "OAUTH_REDIRECT_URL=${cfg.redirectUrl}"
              # Where the embedder downloads and loads its model (UForm v3 ONNX).
              "EMBEDDING_CACHE_DIR=/var/lib/travelai/models"
              # `ort` uses load-dynamic: dlopen libonnxruntime from here at runtime.
              "ORT_DYLIB_PATH=${pkgs.onnxruntime}/lib/libonnxruntime.so"
              # Writable, persistent dir for downloaded activity images (content-
              # addressed) — the Nix store WorkingDirectory is read-only.
              "IMAGE_STORE_DIR=/var/lib/travelai/images"
            ]
            ++ lib.optionals (cfg.otelEndpoint != "") [
              "OTEL_EXPORTER_OTLP_ENDPOINT=${cfg.otelEndpoint}"
              "OTEL_SERVICE_NAME=travelai"
            ]
            ++ lib.optionals cfg.managePostgres [
              # Peer auth over the local Unix socket — the "travelai" system
              # user maps to the "travelai" role, no password needed.
              "DATABASE_URL=postgres://travelai@/${cfg.databaseName}?host=/run/postgresql"
            ];
            Restart = "on-failure";
            RestartSec = "10s";
          };

          script = "${cfg.package}/bin/travelai";
        };

        users.users.travelai = {
          isSystemUser = true;
          group = "travelai";
        };
        users.groups.travelai = {};

        networking.firewall.allowedTCPPorts = [cfg.port] ++ lib.optionals cfg.enableTLS [443];
      }

      (lib.mkIf cfg.managePostgres {
        services.postgresql = {
          enable = true;
          extraPlugins = with config.services.postgresql.package.pkgs; [postgis];
          ensureDatabases = [cfg.databaseName];
          ensureUsers = [
            {
              name = "travelai";
              ensureDBOwnership = true;
            }
          ];
        };
      })
    ]);
}
