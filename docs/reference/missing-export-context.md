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
Display All Missing Files opens the existing main-window repair manager and
stops this sync. Its scope remains all missing collection files, with the
existing selection, protection, Relocate and Auto Relocate behavior. Opening
it does not choose files or run repairs. Retry sync explicitly after repairing.
The native handoff targets only the main window and waits for its displayed
dialog acknowledgment before closing the preflight; a missing owner or failed
module load leaves the prompt usable. Duplicate requests and stale replies
are refused. The browser uses the same App-owned manager. No automatic resume
or export-only repair selection is introduced.

Native regressions cover overlapping selected playlists, repeated track
entries, folder plus explicitly selected child, intelligent membership,
unselected-playlist exclusion, distinct identities sharing metadata, bounded
ancestry and existence-check count. The German browser fixture checks literal
track/path/playlist labels after the translated catalogue has loaded, distinct
rows and unchanged Cancel/Continue dispatch. These are disposable fixture and
application-browser checks, not physical USB or native-window observations.
