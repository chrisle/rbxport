# Missing audio before USB sync

The preflight lists each missing collection track once, with its expected audio
path and the containing playlists in the requested export selection. The track
ID remains its identity when different tracks have the same title or path.
Repeated occurrences within one selected playlist produce one playlist label;
other selected playlists containing the same track remain listed separately.
Folder selections expand to their selected descendants, intelligent playlists
use their already evaluated export membership, and unselected playlists are
not queried or returned. Folder breadcrumbs distinguish same-named playlists.

Attribution reads the existing `ExportSelection` track indices and selected
playlist rows. It does not query the library for each missing track, reevaluate
smart rules or scan the filesystem for memberships. The existing existence
check runs once for each unique selected track; a selection with no missing
tracks does not traverse memberships again. Invalid indices and malformed
ancestor cycles cannot make the attribution loop unbounded.

Cancel stops before export. Keep available copies and sync preserves the
existing behavior: usable audio already on each USB is retained, and missing
tracks without usable copies are skipped. This dialog does not relocate files.
The existing repair route is to cancel, return to the main window, use
File > Display All Missing Files and its Relocate/Auto Relocate commands, then
retry sync. No new cross-window handoff or automatic resume is introduced.

Native regressions cover overlapping selected playlists, repeated track
entries, folder plus explicitly selected child, intelligent membership,
unselected-playlist exclusion, distinct identities sharing metadata, bounded
ancestry and existence-check count. The German browser fixture checks literal
track/path/playlist labels after the translated catalogue has loaded, distinct
rows and unchanged Cancel/Continue dispatch. These are disposable fixture and
application-browser checks, not physical USB or native-window observations.
