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
        bin = pkgs.callPackage ./packaging/nix/bin.nix { };
      in
      {
        packages = {
          inherit bin;
          default = bin;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cargo
            cargo-tauri
            clippy
            nodejs_24
            perl
            pkg-config
            pnpm_10
            rustc
            rustfmt
            wrapGAppsHook3
          ];

          buildInputs = with pkgs; [
            alsa-lib
            at-spi2-atk
            atk
            cairo
            gdk-pixbuf
            glib
            glib-networking
            gsettings-desktop-schemas
            gtk3
            libappindicator-gtk3
            librsvg
            libsoup_3
            openssl
            pango
            sqlite
            webkitgtk_4_1
          ];
        };
      }
    );
}
