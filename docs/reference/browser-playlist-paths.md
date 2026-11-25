# Browser playlist paths

For one selected collection track, the browser header's dropdown lists Collection
and that track's containing playlists. Folder paths distinguish playlists with
the same name; names remain literal data under interface-language translation.
The existing lookup includes ordinary and intelligent playlists and excludes
folders as membership destinations.

Choosing a path opens it in the same browser and locates the selected track,
scrolling to its first position when needed. Selection remains the existing
content-ID selection: duplicate occurrences share the highlight. Main and
secondary browsers keep independent sources, selection and navigation.

The control is disabled for no selection, multiple selections, a selected row
not resolved in the current view, and device or loose-file rows. A lookup error
is reported through the existing status log. Answers from an earlier selection,
view or library are hidden. Navigation cannot overwrite a newer selection after
a delayed answer. Nothing in this route edits a library or a playlist.

The placement and workflow follow the rekordbox 7.2.18 manual's EXPORT browser
panel, page81 item4, and the installed7.2.19 header path-combobox and selected-ID
restoration paths. This implementation covers the existing collection playlist
lookup. The reference's other path families (iTunes, histories and sampler) are
not added here. It does not add a hover interaction or change the loop controls
also mentioned in issue825.
