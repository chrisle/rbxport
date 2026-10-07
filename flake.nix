{
  description = "Rekordbox-compatible export-mode library manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        rbxport = pkgs.callPackage ./packaging/nix/package.nix { };
      in
      {
        packages = {
          inherit rbxport;
          default = rbxport;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ rbxport ];
          packages = with pkgs; [
            cargo-tauri
            clippy
            rustfmt
          ];
        };
      }
    );
}
