{
  lib,
  pkgs,
  rustPlatform,
  pkg-config,
  openssl,
  nodejs,
  enableTLS ? false,
  basePath ? "./",
}: let
  frontend = pkgs.buildNpmPackage {
    pname = "travelai-frontend";
    version = "0.1.0";

    src = lib.cleanSourceWith {
      src = ./frontend;
      filter = path: type:
        !(lib.hasInfix "/node_modules/" path)
        && !(lib.hasInfix "/dist/" path);
    };

    npmDepsHash = "sha256-Dac7HiSiqYFL+X+kAhEXudsMAdZPENIKEk9rLUHmEY0=";

    npmFlags = ["--legacy-peer-deps"];

    nativeBuildInputs = [nodejs];

    env.API_BASE_PATH = basePath;

    buildPhase = ''
      runHook preBuild
      npm run build -- --base=${basePath}
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      mkdir -p $out
      cp -r dist/. $out/
      runHook postInstall
    '';
  };
in
  rustPlatform.buildRustPackage rec {
    pname = "travelai";
    version = "0.1.0";

    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: type:
        (lib.hasSuffix ".rs" path)
        || (baseNameOf path == "Cargo.toml")
        || (baseNameOf path == "Cargo.lock")
        || (!(lib.hasSuffix ".nix" path) && !(lib.hasSuffix ".md" path) && !(lib.hasPrefix "frontend" path));
    };

    cargoLock.lockFile = ./Cargo.lock;

    nativeBuildInputs = [pkg-config];
    buildInputs = [openssl];

    buildPhase = ''
      runHook preBuild
      cargo build --release ${lib.optionalString (!enableTLS) "--no-default-features --features=http"}
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall

      mkdir -p $out/bin
      install -m755 -T target/release/travelai $out/bin/travelai

      mkdir -p $out/bin/frontend/dist
      cp -r ${frontend}/. $out/bin/frontend/dist/

      runHook postInstall
    '';

    env = {
      OPENSSL_DIR = "${openssl.dev}";
      OPENSSL_LIB_DIR = "${openssl.out}/lib";
    };

    meta = with lib; {
      description = "Intelligent paragliding and outdoor adventure travel planning CLI";
      homepage = "https://github.com/thriemer/paragliding-calendar";
      license = licenses.mit;
      platforms = platforms.linux;
    };
  }
