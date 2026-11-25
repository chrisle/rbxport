# Explorer folder import

Right-click an Explorer directory and choose **Import as Playlist** to import
its audio into a playlist named after the directory, under the Playlists root.
The main and secondary browser use the same command. The Explorer heading and
device libraries do not acquire this command.

This route uses the existing folder importer, including recursive audio
collection, automatic analysis, status feedback and Preferences' same-name
folder policy. Library Protection and an open rekordbox library disable the
command. No new collision policy, import format or keyboard binding is added.

The installed rekordbox 7.2.19 ARM64 tree menu contains `strImportAsPlaylist`
(command -123). `TreeViewer::treeMessageImportAsPlaylist` requires a folder,
uses its name, checks for a same-named destination and calls
`BrowseBasicView::importDirToPlaylist`. Its destination is the Playlists root
stored by `TreeViewer::addPlaylistRoot`. This is static source evidence;
no native window interaction or physical USB acceptance is claimed here.

`explorer-folder-playlist.spec.ts` checks both browsers, root placement,
same-name copies and write guards through the application. The existing native
`a_folder_dropped_on_the_playlists_root_becomes_a_playlist` fixture checks the
shared importer with disposable WAV files, a nested directory and collision
handling. The folder replacement fixtures check undo and external-change
refusal. This covers the Explorer action in #827; missing-source playlist
attribution and relocation during USB sync remain separate concerns.
