{
  description = "TravelAI - Intelligent paragliding and outdoor adventure travel planning";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    bun2nix = {
      url = "github:nix-community/bun2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    bun2nix,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [bun2nix.overlays.default];
        };
      in {
        packages.travelai-tls = pkgs.callPackage ./package.nix {enableTLS = true;};
        packages.travelai-http = pkgs.callPackage ./package.nix {enableTLS = false;};
        packages.default = self.packages.${system}.travelai-http;
        nixosModules.travelai = import ./module.nix {inherit self;};

        devShells.default = pkgs.mkShell {
          buildInputs = [pkgs.bun2nix pkgs.bun];
        };
      }
    );
}
