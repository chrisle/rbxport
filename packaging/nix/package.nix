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
  pname = "rbxport";
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

  desktopItem = makeDesktopItem {
    name = pname;
    desktopName = pname;
    comment = "DJ library and USB export manager";
    exec = pname;
    icon = pname;
    startupWMClass = pname;
    terminal = false;
    categories = [
      "Audio"
      "Music"
    ];
  };
in
rustPlatform.buildRustPackage {
  inherit pname version src;

  cargoLock = {
    lockFile = ../../Cargo.lock;
    outputHashes = {
      "alphatheta-connect-0.25.3" = "sha256-Rwgo3c1eoqF5d4X/gsR5/0zSB0NicbIRuZMKf8meA+g=";
    };
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

  postInstall = ''
    install -Dm644 "${desktopItem}/share/applications/rbxport.desktop" \
      "$out/share/applications/rbxport.desktop"

    install -Dm644 "$src/src-tauri/icons/32x32.png" \
      "$out/share/icons/hicolor/32x32/apps/rbxport.png"
    install -Dm644 "$src/src-tauri/icons/128x128.png" \
      "$out/share/icons/hicolor/128x128/apps/rbxport.png"
    install -Dm644 "$src/src-tauri/icons/128x128@2x.png" \
      "$out/share/icons/hicolor/256x256@2/apps/rbxport.png"

    rm -f "$out"/lib/librbxport_lib.*
    rmdir "$out/lib" 2>/dev/null || true
  '';

  meta = {
    description = "Rekordbox-compatible export-mode library manager";
    longDescription = ''
      Manage a DJ library, analyze tracks, write USB exports, and serve a
      library to supported players over a local network.
    '';
    homepage = "https://rbxport.com";
    license = lib.licenses.gpl2Plus;
    mainProgram = "rbxport";
    platforms = lib.platforms.linux;
  };
}
