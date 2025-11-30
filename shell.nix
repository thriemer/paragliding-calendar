{pkgs ? import <nixpkgs> {}}:
pkgs.mkShell rec {
  buildInputs = with pkgs; [
    expat
    bluez
    zenity
    udev
    fontconfig
    freetype
    dbus
    freetype.dev
    libGL
    pkg-config
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    wayland
    libxkbcommon
    python312
    zlib
    openssl
    openssl.dev
  ];

  LD_LIBRARY_PATH =
    builtins.foldl' (a: b: "${a}:${b}/lib") "${pkgs.vulkan-loader}/lib" buildInputs;

  shellHook = ''
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export OPENSSL_DIR="${pkgs.openssl.dev}"
    export OPENSSL_LIB_DIR="${pkgs.openssl.out}/lib"
    export OPENSSL_INCLUDE_DIR="${pkgs.openssl.dev}/include"
    echo "To patch your binary after building, run:"
    echo 'alias run="cargo build && patchelf --set-interpreter $(cat $NIX_CC/nix-support/dynamic-linker) --set-rpath ${pkgs.lib.makeLibraryPath buildInputs} target/debug/prod_tool && ./target/debug/prod_tool"'
  '';
}
