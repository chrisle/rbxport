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
        pkgs = import nixpkgs { inherit system; };
        source = pkgs.callPackage ./packaging/nix/source.nix { };
        bin = pkgs.callPackage ./packaging/nix/bin.nix { };
      in
      {
        packages = {
          inherit source bin;
          default = bin;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ source ];
          packages = with pkgs; [
            cargo-tauri
            clippy
            rustfmt
          ];
        };
      }
    );
}
