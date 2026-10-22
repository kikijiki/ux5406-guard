{
  description = "Thermal guard for the ASUS Zenbook S 14 UX5406SA";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      package = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "ux5406-guard";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta.mainProgram = "ux5406-guard";
      };
    in
    {
      packages = forAll (pkgs: { default = package pkgs; });
      checks = forAll (pkgs: { default = package pkgs; });
      devShells = forAll (pkgs: {
        default = pkgs.mkShell { packages = [ pkgs.cargo pkgs.rustc pkgs.clippy pkgs.rustfmt ]; };
      });

      nixosModules.default = import ./nixos-module.nix self;
    };
}
