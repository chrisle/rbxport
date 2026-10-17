{
  lib,
  rustPlatform,
  fetchPnpmDeps,
  makeDesktopItem,
  pnpm_10,
  pnpmConfigHook,
  nodejs_24,
  pkg-config,
  perl,
  wrapGAppsHook3,
  webkitgtk_4_1,
  gtk3,
  libappindicator-gtk3,
  librsvg,
  alsa-lib,
  libsoup_3,
  glib,
  glib-networking,
  gsettings-desktop-schemas,
  at-spi2-atk,
  atk,
  cairo,
  pango,
  gdk-pixbuf,
  sqlite,
  openssl,
}:

let
  common = import ./common.nix { inherit lib makeDesktopItem; };

  pname = common.pname;
  version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
  pin = builtins.fromJSON (builtins.readFile ./pin.json);

  src = lib.cleanSourceWith {
    src = ../..;
    filter =
      path: _type:
      let
        base = baseNameOf path;
      in
      !(builtins.elem base [
        ".git"
        "node_modules"
        "target"
        "dist"
        "dist-mock"
        "test-results"
        "playwright-report"
      ]);
  };
in
rustPlatform.buildRustPackage {
  inherit pname version src;

  cargoLock = {
    lockFile = ../../Cargo.lock;
    outputHashes = pin.source.cargoOutputHashes;
  };

  cargoBuildFlags = [
    "-p"
    pname
  ];

  buildFeatures = [ "tauri/custom-protocol" ];

  pnpmDeps = fetchPnpmDeps {
    inherit pname version src;
    pnpm = pnpm_10;
    fetcherVersion = 4;
    hash = pin.source.pnpmDepsHash;
  };

  nativeBuildInputs = [
    nodejs_24
    pnpm_10
    pnpmConfigHook
    pkg-config
    perl
    wrapGAppsHook3
  ];

  buildInputs = [
    webkitgtk_4_1
    gtk3
    libappindicator-gtk3
    librsvg
    alsa-lib
    libsoup_3
    glib
    glib-networking
    gsettings-desktop-schemas
    at-spi2-atk
    atk
    cairo
    pango
    gdk-pixbuf
    sqlite
    openssl
  ];

  doCheck = false;

  preBuild = ''
    pnpm build
  '';

  preFixup = ''
    gappsWrapperArgs+=(--set RBXPORT_PACKAGE_MANAGER nix)
  '';

  postInstall = common.installDesktop + ''
    rm -f "$out"/lib/librbxport_lib.*
    rmdir "$out/lib" 2>/dev/null || true
  '';

  meta = common.meta;
}
