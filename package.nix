{
  lib,
  pkgs,
  rustPlatform,
  pkg-config,
  openssl,
  bun,
  enableTLS ? false,
}:
rustPlatform.buildRustPackage rec {
  pname = "travelai";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
    # Keep Rust source files, Cargo.toml/lock, and frontend assets
      (lib.hasSuffix ".rs" path)
      || (baseNameOf path == "Cargo.toml")
      || (baseNameOf path == "Cargo.lock")
      || (lib.hasPrefix "frontend" path)
      ||
      # But exclude NixOS module files when building the Rust package
      (!(lib.hasSuffix ".nix" path) && !(lib.hasSuffix ".md" path));
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    bun
    pkgs.bun2nix.hook # the magic setup hook
  ];

  buildInputs = [openssl];

  bunRoot = "frontend";
  bunDeps = pkgs.bun2nix.fetchBunDeps {
    bunNix = ./bun.nix; # generated in step 2
  };

  buildPhase = ''
    runHook preBuild

    # Build frontend
    cd frontend
    bun install --frozen-lockfile
    bun run build
    cd ..

    # Build Rust binary
    cargo build --release ${lib.optionalString (!enableTLS) "--no-default-features --features=http"}

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin
    mkdir -p $out/bin/frontend/dist

    # Install binary
    install -m755 -T target/release/travelai $out/bin/travelai

    # Install frontend
    cp -r frontend/dist $out/bin/frontend/dist

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
