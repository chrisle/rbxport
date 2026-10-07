{
  lib,
  makeDesktopItem,
}:

let
  pname = "rbxport";

  icon = file: "${../..}/src-tauri/icons/${file}";

  desktopItem = makeDesktopItem {
    name = pname;
    desktopName = pname;
    comment = "DJ library and USB export manager";
    exec = pname;
    icon = pname;
    startupWMClass = pname;
    terminal = false;
    categories = [
      "AudioVideo"
      "Audio"
      "Music"
    ];
  };
in
{
  inherit pname icon desktopItem;

  installDesktop = ''
    install -Dm644 ${desktopItem}/share/applications/rbxport.desktop \
      "$out/share/applications/rbxport.desktop"
    install -Dm644 ${icon "32x32.png"} "$out/share/icons/hicolor/32x32/apps/rbxport.png"
    install -Dm644 ${icon "128x128.png"} "$out/share/icons/hicolor/128x128/apps/rbxport.png"
    install -Dm644 ${icon "128x128@2x.png"} "$out/share/icons/hicolor/256x256@2/apps/rbxport.png"
  '';

  meta = {
    description = "Rekordbox-compatible export-mode library manager";
    longDescription = ''
      Manage a DJ library, analyze tracks, write USB exports, and serve a
      library to supported players over a local network.
    '';
    homepage = "https://rbxport.com";
    license = lib.licenses.gpl2Plus;
    mainProgram = pname;
    platforms = lib.platforms.linux;
  };
}
