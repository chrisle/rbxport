{
  lib,
  stdenv,
  fetchurl,
  writeShellApplication,
  makeDesktopItem,
  appimage-run,
  coreutils,
}:

let
  common = import ./common.nix { inherit lib makeDesktopItem; };

  version = "1.0.0";

  appimage = fetchurl {
    url = "https://download.rbxport.com/rbxport-${version}-linux-x86_64.AppImage";
    hash = "sha256-LjEARgBBO/cdJtcxcLH8WHfHQ73YsTgZeimX5nPap0Y=";
  };

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
      mutable="$data/rbxport.AppImage"
      marker="$data/.nix-seed-version"
      seed_version="@version@"

      # The built-in updater replaces the AppImage with rename(2), which fails
      # with EXDEV when the temp directory and the AppImage are on different
      # mounts (as /tmp is inside appimage-run's sandbox), so keep temp files
      # beside the AppImage it is replacing.
      tmpdir="$data/tmp"
      mkdir -p "$tmpdir"
      export TMPDIR="$tmpdir"

      # Seed the writable AppImage the built-in updater replaces in place,
      # but only when it is missing. An existing one is kept as-is, so a Nix
      # update cannot downgrade an app the updater has already advanced; the
      # app decides for itself whether it is outdated.
      if [ ! -e "$mutable" ] || [ ! -e "$marker" ]; then
        mkdir -p "$data"
        cp -L "@seed@" "$mutable"
        chmod u+w "$mutable"
      fi
      printf '%s' "$seed_version" > "$marker"

      # Patch the AppImage by dropping the its bundled GTK/WebKit, 
      # if it hasn't already. The generic stack has troubles with
      # EGL display creation on Nix/NixOS, so the host
      # libraries (provided by appimage-run) are used instead.
      hash="$(sha256sum "$mutable" | cut -d' ' -f1)"
      extracted="$cache/$hash"
      if [ ! -e "$extracted/.nix-ready" ]; then
        mkdir -p "$cache"
        rm -rf "$extracted"
        ${lib.getExe appimageRunner} -x "$extracted" "$mutable"
        rm -rf "$extracted/usr/lib"
        # The bundled GTK hook forces its own GTK_THEME and schemas, so the
        # desktop's theme is ignored. Empty it out (AppRun still sources it).
        mkdir -p "$extracted/apprun-hooks"
        : > "$extracted/apprun-hooks/linuxdeploy-plugin-gtk.sh"
        touch "$extracted/.nix-ready"
      fi

      APPIMAGE="$mutable" exec ${lib.getExe appimageRunner} -w "$extracted" -- "$@"
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
      --replace-fail '@seed@' "$out/share/rbxport/rbxport.AppImage" \
      --replace-fail '@version@' "${version}"

    runHook postInstall
  '';

  meta = common.meta // {
    description = "Rekordbox-compatible export-mode library manager (AppImage)";
    longDescription = common.meta.longDescription + ''

      This build wraps the official AppImage. On first launch it is copied to
      a writable location under the user's data directory so the built-in
      updater can replace it in place.
    '';
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
  };
}
