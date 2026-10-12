let
  pin = builtins.fromJSON (builtins.readFile ./pin.json);
in
{
  lib,
  stdenv,
  fetchurl,
  writeShellApplication,
  makeDesktopItem,
  appimage-run,
  coreutils,
  # Overridable so CI can smoke-test a locally built release AppImage before
  # it is published and the pin is bumped.
  appimage ? fetchurl {
    url = "https://download.rbxport.com/rbxport-${pin.version}-linux-x86_64.AppImage";
    hash = pin.hash;
  },
}:

let
  common = import ./common.nix { inherit lib makeDesktopItem; };

  version = pin.version;

  appimageRunner = appimage-run.override {
    extraPkgs = pkgs: [
      pkgs.webkitgtk_4_1
      pkgs.libsoup_3
      pkgs.libappindicator-gtk3
      pkgs.glib-networking
    ];
  };

  launcher = writeShellApplication {
    name = common.pname + "-launcher";
    runtimeInputs = [ coreutils ];
    text = ''
      data="''${XDG_DATA_HOME:-$HOME/.local/share}/rbxport"
      cache="''${XDG_CACHE_HOME:-$HOME/.cache}/rbxport"
      appimage="@appimage@"
      key="@key@"

      # Nix owns this package's updates: the app runs the AppImage in the
      # store, which nothing but Nix can replace, and this variable makes its
      # built-in updater unavailable and point at the Nix channel instead.
      export RBXPORT_PACKAGE_MANAGER=nix

      # The first Nix launcher seeded a writable copy here for the built-in
      # updater to replace, which let a Nix install update outside Nix.
      # Remove what it left; only it wrote the marker.
      if [ -e "$data/.nix-seed-version" ]; then
        rm -rf "$data/rbxport.AppImage" "$data/tmp" "$data/.nix-seed-version"
        rmdir "$data" 2>/dev/null || true
      fi

      # Extract each package once, dropping the AppImage's bundled GTK/WebKit:
      # that generic stack cannot create an EGL display on NixOS, so the host
      # libraries (provided by appimage-run) are used instead.
      extracted="$cache/$key"
      if [ ! -e "$extracted/.nix-ready" ]; then
        mkdir -p "$cache"
        rm -rf "$extracted"
        ${lib.getExe appimageRunner} -x "$extracted" "$appimage"
        rm -rf "$extracted/usr/lib"
        # The bundled GTK hook forces its own GTK_THEME and schemas, so the
        # desktop's theme is ignored. Empty it out (AppRun still sources it).
        mkdir -p "$extracted/apprun-hooks"
        : > "$extracted/apprun-hooks/linuxdeploy-plugin-gtk.sh"
        touch "$extracted/.nix-ready"
      fi

      APPIMAGE="$appimage" exec ${lib.getExe appimageRunner} -w "$extracted" -- "$@"
    '';
  };
in
stdenv.mkDerivation {
  pname = common.pname + "-bin";

  inherit version;

  dontUnpack = true;

  installPhase = ''
    runHook preInstall

    install -Dm555 ${appimage} "$out/share/rbxport/rbxport.AppImage"
    ${common.installDesktop}

    install -Dm755 ${lib.getExe launcher} "$out/bin/rbxport"
    substituteInPlace "$out/bin/rbxport" \
      --replace-fail '@appimage@' "$out/share/rbxport/rbxport.AppImage" \
      --replace-fail '@key@' "$(basename "$out")"

    runHook postInstall
  '';

  meta = common.meta // {
    description = "Rekordbox-compatible export-mode library manager (AppImage)";
    longDescription = common.meta.longDescription + ''

      This build wraps the official AppImage and runs it from the Nix store.
      Updates come through Nix; the app's built-in updater is unavailable.
    '';
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
  };
}
