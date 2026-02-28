{ lib
, fetchFromGitHub
, rustPlatform
, pkg-config
, openssl
, bun
, enableTLS ? true
}:

rustPlatform.buildRustPackage rec {
  pname = "travelai";
  version = "0.1.0";

  src = lib.cleanSource ./.;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

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
    mkdir -p $out/share/travelai/frontend

    # Install binary
    install -m755 -T target/release/travelai $out/bin/travelai

    # Install frontend
    cp -r frontend/dist $out/share/travelai/frontend

    runHook postInstall
  '';

  env = {
    OPENSSL_DIR = "${openssl.dev}";
    OPENSSL_LIB_DIR = "${openssl.out}/lib";
  };

  meta = with lib; {
    description = "Intelligent paragliding and outdoor adventure travel planning CLI";
    homepage = "https://github.com/anomalyco/travelai";
    license = licenses.mit;
    platforms = platforms.linux;
  };
}
