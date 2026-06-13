{
  description = "TravelAI - Intelligent paragliding and outdoor adventure travel planning";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {inherit system;};
      in {
        packages.travelai-tls = pkgs.callPackage ./package.nix {enableTLS = true;};
        packages.travelai-http = pkgs.callPackage ./package.nix {enableTLS = false;};
        packages.default = self.packages.${system}.travelai-http;
        nixosModules.travelai = import ./module.nix {inherit self;};

        devShells.default = pkgs.mkShell {
          buildInputs = [pkgs.nodejs];
        };
      }
    );
}
