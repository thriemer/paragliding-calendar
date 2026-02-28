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

    secretsFilePath = lib.mkOption {
      type = lib.types.path;
      description = "Path to the secrets of paragliding calendar.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.travelai.package = lib.mkDefault (
      if cfg.enableTLS
      then self.packages.${pkgs.system}.travelai-tls
      else self.packages.${pkgs.system}.travelai-http
    );

    systemd.services.travelai = {
      description = "TravelAI - Paragliding and outdoor adventure planning";
      wantedBy = ["multi-user.target"];
      after = ["network.target"];

      serviceConfig = {
        Type = "simple";
        User = "travelai";
        Group = "travelai";
        WorkingDirectory = "${cfg.package}/bin";
        EnvironmentFile = "${cfg.secretsFilePath}";
        Environment = [
          "PORT=${toString cfg.port}"
          "RUST_LOG=${cfg.logLevel}"
        ];
        CacheDirectory = "travelai";
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
  };
}
