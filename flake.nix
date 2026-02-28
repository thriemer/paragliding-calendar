{
  description = "TravelAI - Intelligent paragliding and outdoor adventure travel planning";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ ];
        };
      in
      {
        packages.travelai = pkgs.callPackage ./package.nix { };

        packages.default = self.packages.${system}.travelai;

        nixosModules.travelai = import ./module.nix;
      }
    );
}
