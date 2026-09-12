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
  };

  config = lib.mkIf cfg.enable (let
    travelai = pkgs.callPackage ./package.nix {
      enableTLS = cfg.enableTLS;
      basePath = cfg.basePath;
    };
  in {
    services.travelai.package = travelai;

    systemd.services.travelai = {
      description = "TravelAI - Paragliding and outdoor adventure planning";
      wantedBy = ["multi-user.target"];
      after = ["network.target" "postgresql.service"];

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
        ] ++ lib.optionals (cfg.otelEndpoint != "") [
          "OTEL_EXPORTER_OTLP_ENDPOINT=${cfg.otelEndpoint}"
          "OTEL_SERVICE_NAME=travelai"
        ];
        Restart = "on-failure";
        RestartSec = "10s";
      };

      script = "${cfg.package}/bin/travelai";
    };

    users.users.travelai = lib.mkIf cfg.enable {
      isSystemUser = true;
      group = "travelai";
    };
    users.groups.travelai = lib.mkIf cfg.enable {};

    networking.firewall.allowedTCPPorts = [cfg.port] ++ lib.optionals cfg.enableTLS [443];
  });
}
