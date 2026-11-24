/** Established native commands shared by menu routing and the unbound Keyboard rows. */
export const NATIVE_COMMANDS = [
  ["importToCollection", "400f", "Import To Collection", "Track"],
  ["analysisLockOn", "4012", "Analysis Lock On", "Track"],
  ["analysisLockOff", "4013", "Analysis Lock Off", "Track"],
  ["reloadTag", "4003", "Reload Tag", "Track"],
  ["removeFromCollection", "400b", "Remove from Collection", "Track"],
  ["autoRelocate", "4014", "Auto Relocate", "Track"],
  ["relocate", "400e", "Relocate", "Track"],
  ["createPlaylist", "500a", "Create New Playlist", "Playlist"],
  ["createIntelligentPlaylist", "500b", "Create New Intelligent Playlist", "Playlist"],
  ["createFolder", "500c", "Create New Folder", "Playlist"],
  ["deletePlaylist", "5004", "Delete Playlist", "Playlist"],
  ["exportPlaylistTxt", "5005", "Export a playlist to a file (*.txt)", "Playlist"],
  ["exportPlaylistM3u8", "5006", "Export a playlist to a file for music apps (*.m3u8)", "Playlist"],
  ["importPlaylist", "5009", "Import Playlist", "Playlist"],
] as const;

export function nativeKeyRows(group: "Track" | "Playlist") {
  return NATIVE_COMMANDS.filter(row => row[3] === group).map(([, id, label]) => ({ id, label, key: null }));
}
