{ config, lib, pkgs, ... }:

let
  cfg = config.services.travelai;
in
{
  options.services.travelai = {
    enable = lib.mkEnableOption "travelai - Intelligent paragliding and outdoor adventure travel planning";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The travelai package to use.";
      default = pkgs.travelai;
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "Port to listen on.";
    };

    enableTLS = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable TLS support. Requires tlsCertPath and tlsKeyPath.";
    };

    tlsCertPath = lib.mkOption {
      type = lib.types.path;
      default = "/etc/travelai/cert.pem";
      description = "Path to TLS certificate file.";
    };

    tlsKeyPath = lib.mkOption {
      type = lib.types.path;
      default = "/etc/travelai/key.pem";
      description = "Path to TLS private key file.";
    };

    googleClientId = lib.mkOption {
      type = lib.types.str;
      description = "Google OAuth client ID.";
    };

    googleClientSecret = lib.mkOption {
      type = lib.types.str;
      description = "Google OAuth client secret.";
    };

    gmailAddress = lib.mkOption {
      type = lib.types.str;
      description = "Gmail address for sending notifications.";
    };

    gmailAppPassword = lib.mkOption {
      type = lib.types.str;
      description = "Gmail app password for sending notifications.";
    };

    notificationEmail = lib.mkOption {
      type = lib.types.str;
      description = "Email address to send notifications to.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.travelai = {
      description = "TravelAI - Paragliding and outdoor adventure planning";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      serviceConfig = {
        Type = "simple";
        User = "travelai";
        Group = "travelai";
        WorkingDirectory = "${cfg.package}/share/travelai";

        Environment = [
          "PORT=${toString cfg.port}"
          "GOOGLE_CLIENT_ID=${cfg.googleClientId}"
          "GOOGLE_CLIENT_SECRET=${cfg.googleClientSecret}"
          "GMAIL_ADDRESS=${cfg.gmailAddress}"
          "GMAIL_APP_PASSWORD=${cfg.gmailAppPassword}"
          "NOTIFICATION_EMAIL=${cfg.notificationEmail}"
        ];

        EnvironmentFile = lib.optionals cfg.enableTLS [
          "TLS_CERT_PATH=${cfg.tlsCertPath}"
          "TLS_KEY_PATH=${cfg.tlsKeyPath}"
        ];

        Restart = "on-failure";
        RestartSec = "10s";
      };

      script = "${cfg.package}/bin/travelai";
    };

    users.users.travelai = lib.mkIf (config.users.users.travelai == {}) {
      isSystemUser = true;
      group = "travelai";
    };

    users.groups.travelai = lib.mkIf (config.users.groups.travelai == {}) {};

    networking.firewall.allowedTCPPorts = [ cfg.port ] ++ lib.optionals cfg.enableTLS [ 443 ];
  };
}
