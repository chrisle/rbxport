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

  version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;

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
  pname = common.pname + "-source";
  inherit version src;

  cargoLock = {
    lockFile = ../../Cargo.lock;
    outputHashes = {
      "alphatheta-connect-0.25.3" = "sha256-Rwgo3c1eoqF5d4X/gsR5/0zSB0NicbIRuZMKf8meA+g=";
    };
  };

  cargoBuildFlags = [
    "-p"
    common.pname
  ];

  buildFeatures = [ "tauri/custom-protocol" ];

  pnpmDeps = fetchPnpmDeps {
    pname = common.pname;
    inherit version src;
    pnpm = pnpm_10;
    fetcherVersion = 4;
    hash = "sha256-D9yTAZ/D4+oZfRtPhk9NvNFweU1o7qe6z7HJkNjmP2w=";
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

  postInstall = common.installDesktop + ''
    rm -f "$out"/lib/librbxport_lib.*
    rmdir "$out/lib" 2>/dev/null || true
  '';

  meta = common.meta;
}
