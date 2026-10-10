# CI entry point for the Nix smoke test.
#
# Builds `bin`, optionally replacing the pinned seed with a locally built
# AppImage passed through RBXPORT_APPIMAGE. Requires `nix build --impure`
# because it reads a flake and the environment.
let
  self = builtins.getFlake (toString ./../..);
  pkgs = self.inputs.nixpkgs.legacyPackages.${builtins.currentSystem};
  appimage = builtins.getEnv "RBXPORT_APPIMAGE";
in
if appimage == "" then
  pkgs.callPackage ./bin.nix { }
else
  pkgs.callPackage ./bin.nix {
    appimage = builtins.path {
      path = builtins.toPath appimage;
      name = "rbxport-ci.AppImage";
    };
  }
