# Changelog

What changed for the person using the app, release by release. Versions are
the git tags; a tag is what the release workflow builds and publishes. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the version numbers [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- Export Folder now writes the folder with its playlists and intelligent
  playlists inside it. Previously the device got one empty playlist named
  after the folder.
- Sync Manager lists intelligent playlists, and ticking a folder includes
  them. Playlists and folders created while Sync Manager is open now appear
  without reopening it.

## [1.2.0] — 2026-10-08

### Added
- Remember the DUAL control across restarts.
- Add keyboard bindings for hot cues D-H and all hot cue clears.
- Preview a track by clicking its waveform in the browser.
- Add AIFF export conversion.
- Add Keyboard pane rows for per-deck low/mid/high EQ kill.
- Hide the missing-file manager from users for now.
- Tell reporters their bug description is posted publicly.
- Show reporters a link to follow their bug report.
- Import every identified column from rekordbox's browser layout.
- Sort tracks by every column rekordbox can sort.

### Fixed
- Match rekordbox's Explorer track menu for imported and loose files.
- Base the track menu in the Explorer on each file's import state.
- Name the refused file handle in stale-handle log lines.
- Match rekordbox's BPM + KEY related tracks, half and double tempo included.
- Analyse every selected track, not only those in the row cache.
- Show the Backups preferences fully in the chosen language.
- Let Windows backups flush the database copy.
- Add files already in the library to the playlist they are dropped on.
- Let beat-grid edits confirm tempo changes on macOS and start at zero.
- Answer intelligent playlists on a My Tag.
- Apply Browse font size and line space to the playlist tree.
- Let mouse wheel notches always step the waveform zoom.
- Slow trackpad waveform zoom and make the sidebar easier to resize.
- Install Windows updates silently at quit.
- Sort tracks by DJ play count.
- Use hidden Pioneer root on HFS exports.
- Register analysis for rekordbox waveforms.
- Localize the native menu at runtime.
- Recognize Linux FAT32 sync devices.
- Include intelligent playlists in Sync Manager.

## [1.1.0] — 2026-10-07

### Added
- Warn before unlocking an unbacked library.
- Include complete context in bug reports.

### Fixed
- Default automatic analysis to off.
- Preserve active loop ends on export.
- Tolerate malformed library text.

## [1.0.0] — 2026-10-06

### Added
- rbxport 1.0 is now available.

## [1.0.0-rc.15] — 2026-10-06

### Fixed
- Preserve audio extensions on FAT exports.

## [1.0.0-rc.14] — 2026-10-06

### Added
- Recognize the paired PRO DJ LINK identities used by compatible all-in-one players.

### Fixed
- Files and folders dropped onto a playlist import correctly on Linux.
- Smart Playlists load more reliably from rekordbox XML and match relative-date, text, and numeric rules correctly.
- LINK key labels and key sorting now follow their separate player settings consistently.

### Improved
- Automatic LINK connections prefer wired adapters and keep player membership and addresses current as devices reconnect or leave the network.
- LINK Export file transfers use more player-compatible request sizes and responses, reducing unnecessary network retransmissions during track loads.
- LINK browsing and player control handle unsupported requests, load responses, and rejected connections more reliably.

## [1.0.0-rc.13] — 2026-10-04

### Added
- Automatically connect to available PRO DJ LINK players and mixers with an opt-in setting.
- USB sync warns when a device is not formatted as FAT32.
- Support rbxport with a donation button in the main window.

### Fixed
- RBX-25: Playlist positions remain correct when sorting or filtering. Dropping files onto a playlist now shows import progress immediately.
- RBX-24: Album artwork keeps its original colors in the info panel.
- RBX-23: Create playlists and folders in an empty library or after deleting the last playlist.
- Smart Playlists work in LINK Export, with corrected text and numeric rule matching.
- LINK browsing uses the player's supported categories, preserves playlist hierarchy order, and handles menu pagination correctly.
- XDJ-RX3 browse rows show the active sort column and configured key notation, while preserving key-matching information.
- LINK history shows the active session and records tracks when the player reports them as played.
- Track titles display correctly for legacy LINK clients.
- USB sync refreshes available destinations before starting and repairs legacy analysis files during export.

### Improved
- LINK library services use the selected network interface.
- LINK preview reads use a bounded cache and combine adjacent requests.
- Mixer cue indicators and PRO DJ LINK settings use a more consistent layout.
- Linux uses native window decorations.

## [1.0.0-rc.12] — 2026-09-30

### Added
- Import selected iTunes playlists into your library from the Sync Manager.
- The status bar shows a prompt when an update is ready to install.
- Open rbxport's TikTok from a link in the About pane.
- rbxport reports crashes automatically so failures can be diagnosed and fixed.
- Use LINK with the XDJ-RX3 over rear USB and correctly handle all-in-one players whose decks and mixer share one network address.

### Fixed
- Key analysis now saves its result in new and sparsely imported libraries.
- USB sync on macOS no longer reports a completed export as failed when an exFAT device does not support directory flushing.
- On Linux, USB drives are detected reliably and diagnostics report the app's thread count.

### Improved
- Browsing a player's menus over LINK a second time is instant, as the menus are now cached.
- LINK browsing, sorting, and export compatibility with the XDJ-RX3 are more complete.

## [1.0.0-rc.7] — 2026-09-29

### Added
- Import selected iTunes playlists into your library from the Sync Manager.
- The status bar shows a prompt when an update is ready to install.
- Open rbxport's TikTok from a link in the About pane.
- rbxport reports crashes automatically so failures can be diagnosed and fixed.

### Improved
- Browsing a player's menus over LINK a second time is instant, as the menus are now cached.

## [1.0.0-rc.6] — 2026-09-26

### Added
- Use LINK with the XDJ-RX3 over rear USB and correctly handle all-in-one players whose decks and mixer share one network address.

### Fixed
- USB sync on macOS no longer reports a completed export as failed when an exFAT device does not support directory flushing.

## [1.0.0-rc.5] — 2026-09-26

### Added
- Import cues and beat grids, play history, and CDJ/mixer settings from USB devices with one Import button in the Sync Manager, and choose which are ticked when it opens.

### Fixed
- Over LINK, a player's HISTORY shows this session's LINK HISTORY, as with rekordbox, and tracks played on the decks are recorded to it.
- Adding a track to the Tag List, from the menu or a player, no longer reloads the list on screen.
- Recently added text is translated, and several mistranslated labels are corrected.

## [1.0.0-rc.4] — 2026-09-25

### Fixed
- Rating stars are the same size on macOS, Windows and Linux.
- Scrolling the waveform zoom quickly on linked decks moves through every zoom step instead of skipping some.
- Open and closed folders in the library tree are remembered across restarts.
- On macOS, rbxport asks for access when library music is on a network volume.

## [1.0.0-rc.3] — 2026-09-25

### Added
- Undo and redo playlist renames, moves, deletions and track removals, plus track metadata, rating, comment, colour and My Tag edits.
- See release notes before updating and get a persistent prompt when a downloaded update is ready to install.

### Fixed
- A playing deck immediately plays the next loaded track, even when tracks are switched rapidly.
- The inline BPM editor stays the same compact width as its readout.
- Switching to a shorter playlist no longer leaves old track rows painted below the list.
- USB waveform settings such as RGB can be changed while rekordbox is running; the status badge now clarifies that only library editing is read-only.

### Improved
- Each deck's waveform zoom is remembered across sessions.
- Scrubbing opens to full-frequency audio at normal playback speed.

## [1.0.0-rc.2] — 2026-09-25

### Fixed
- Hot cues without a custom colour use the default green instead of appearing black.

## [1.0.0-rc.1] — 2026-09-24

### Added
- Control rbxport from AppleScript on macOS.
- Undo and redo deleting playlists and folders from the Edit menu.
- Create a rekordbox library when none exists instead of opening an empty window with an error.
- Keep the last library screen, artwork, and waveforms visible while the library loads.
- Right-click the version in the status bar to open the current application log.

### Fixed
- Bug reports can be sent again on macOS and Windows.
- Track artwork appears correctly on Windows.
- Zoomed waveforms line up with the playhead and beat grid on tracks with encoder delay.
- Sub-browser track actions, reordering, and Information now act on the playlist open on the right.

### Improved
- USB devices and Sync Manager load faster on macOS, and Preferences and bug reporting open faster on every platform.

## [0.9.1] — 2026-09-22

### Fixed
- Checking for updates reaches the download server again; it was pointed at
  a location that had gone away.

## [0.9.0] — 2026-09-21

### Changed
- Collaborative playlist is gone from the tree menu with the other cloud
  rows, rather than greyed: there is no cloud library behind this app.
- The rail's ⟳ tip reads Launch Sync Manager, as rekordbox's does.
- Updates install themselves. A new version found by the start-up check is
  downloaded and put in place in the background, with nothing to click, and
  is the one that runs the next time rbxport opens. On Windows the installer
  runs silently when rbxport quits. Help › Check for Updates… still shows
  what is happening and what changed, and offers Restart Now.

### Fixed
- A CDJ browsing a playlist served over LINK no longer sees every track
  greyed as played: only the tracks a player has loaded this session are,
  and a history's rows, as rekordbox greys them.
- A CDJ's list no longer vanishes while it browses: rekordbox never hangs
  up a player's database session, and now neither does this — it lasts
  until the player ends it, with TCP keepalive catching one that vanished.
- Artwork goes to a player at rekordbox's medium size (`artwork_m.jpg`),
  not the full file; sent the full file, a CDJ-3000 left much of its
  list without art.

### Added
- LINK joins the network the way rekordbox 7.2.11 does: nothing is
  announced until a player or mixer is heard, then the number probe —
  three claims and six rounds through 17, 18, 41–44 at 100 ms, taking 17
  or, when another rekordbox holds it, 18 — and a probe of our number is
  answered. The library and the database server open to players only
  once the number is settled, a player can be sent a track only once it
  has mounted the library, and the link goes off with the reason when the
  interface loses its address. Preferences › PRO DJ LINK says which
  device number LINK is on as.
- The file server answers as rekordbox's does: reads up to 64,512 bytes
  and IO at the end of a file, the host's own file attributes and
  filesystem figures, rekordbox's status codes for what cannot be done,
  lock-manager calls dropped, retransmitted calls answered from a cache,
  decomposed (NFD) names on the wire, and handles laid out as its are.
- A log file: everything the app reports goes to stdout and to a daily file
  under the app's data directory (`rbxport/logs`, seven days kept), so a
  LINK session with a player can be read back afterwards. `LOG_LEVEL`
  picks the level (`error`, `warn`, `info`, `debug`, `trace`).
- LINK says what it is doing: every device heard and lost, every player's
  loaded track and master change, every database session a player opens
  and closes, every mount and file it reads, at the level each deserves;
  `trace` adds every packet, request and RPC call.
- A stick carries what rekordbox's does: the artwork at rekordbox's two
  sizes under its names, the My Tags in `exportExt.pdb` for a player's tag
  browsing, and the sync record (`playlists3.sync`) rekordbox's own Sync
  Manager reads back, so a stick written here opens on the same selection
  there.
- Automatic synchronization: a tick under each stick in the Sync Manager.
  A ticked stick is written again with its playlists whenever it is
  plugged in while rbxport is running — and a stick rekordbox synced from
  the same library with its automatic sync on is treated the same way.
- The track menu's Add To Playlist (the playlist tree), Import To
  Collection (over the Explorer's files), Analysis Lock, Reload Tag, Add
  To Tag List and Export Track (to a connected stick, on its own, kept
  there by later syncs); the tree menu's Add Artwork and Add To Shortcut,
  which puts a playlist on the rail as a button of its own (a right
  click on the button offers Delete Shortcut, as rekordbox's does); the
  deck menu's Export Loop As WAV and Export Track.
- The Tag List: rekordbox's temporary list, a section of the tree,
  kept in the library as rekordbox keeps it.
- Track Suggestion under Related Tracks: what followed the loaded track in
  past sets, or what goes with it by BPM and key when nothing did.
- Intelligent playlists are made and edited here: Create New Intelligent
  Playlist and Edit the Intelligent Playlist open a rule editor with
  rekordbox's properties, operators and all/any match.
- File › Import iTunes Library xml… brings Music.app's Library.xml in —
  the files, the playlist folders and lists, ratings and comments.
- Add Artwork writes the picture at the three sizes the library keeps, so
  a stick gets rekordbox's small and medium copies rather than the
  original to scale.
- Preferences › Advanced › Send anonymous usage statistics: on by default,
  an install id with the version and OS once per launch to rbxport.com,
  and nothing about the library. Off means no request at all.
- A Linux build. Each release now ships an AppImage, which the app can
  update itself from, and a .deb, beside the macOS disk image and the
  Windows installer.
- The GRID panel edits the beat grid and saves it to the library: set the
  beat marker, tap the tempo, shift the grid a millisecond either way,
  widen or narrow it by 0.01 BPM, double or halve it, snap the nearest beat
  to the playhead for the whole grid or from the playhead on, undo and
  redo, and lock the grid against edits. CUT sets the point from which the
  other edits apply, so a tempo change part-way through a DJ edit can be
  gridded without moving the beats before it. The keys are rekordbox's:
  command + G opens the panel, command + cursor left/right shift the grid,
  option + command + \ snaps to the playhead. The metronome follows the
  edited grid, and the first time a track's analysis files are rewritten
  the originals are kept under the app's backups.

### Changed
- Analyze reads the whole track instead of the first three minutes. The beat
  grid it reports stays on the beat to the last bar, a tempo change part-way
  through (a DJ edit) gets a second grid with the count running on, and
  beat 1 is chosen from where the music changes — drops, breakdowns, new
  bass lines — rather than assumed to be the first kick. Against the
  155-track RBX-BPM-GRID-TEST playlist the BPM now matches rekordbox on every
  track (was 146), the downbeat on 151 (was 27) and the whole grid on 146
  (was 26). The result is still reported, not written to the library.
- Beats sit on the kick's attack: the start of the click in the 900–9000 Hz
  band, to the millisecond: 0.0 ms from where rekordbox 7 puts them, at
  the median and the 90th percentile.
- A gradual tempo change between two settled tempos is followed bar by
  bar, each bar with its own tempo, rather than held to one cut.
- Key detection uses Ángel Faraldo's edmkey method (as in Essentia) and
  agrees with rekordbox on 141 of those 155 tracks, up from 27. A toss-up
  between a major key and its parallel minor goes to the minor, as it does
  in this library.
- Analyze Track writes its result to the library: the beat grid and every
  waveform go into the analysis files beside rekordbox's own, and the BPM,
  key, length and analysis path onto the track. It is back in the track and
  player menus and on `A`; a track rekordbox analysed keeps its phrases.
- Intelligent playlists open, sort, search and export as the tracks their
  rule admits, with their own icon in the tree.
- A stick carries the tracks' artwork and the library's My Tags, and is
  dated the day it was made where the machine is.
- Loops on the player: AU loops the chosen number of beats from the head,
  snapped to the grid when Q is on; MA takes IN and OUT by hand, and
  RELOOP/EXIT either way. The seam is the audio's own join.
- File › Import rekordbox xml… brings a rekordbox XML collection in — the
  files, the playlist tree, and each new track's rating, comment and cues —
  and Export Collection in xml format… writes one.
- The track menu's Reset DJ Play Count, Remove from Collection (which asks
  first) and Convert Memory Cues to Hot Cues are live, and the tree menu's
  Export a playlist to a file writes an m3u8 or a tab-separated txt.
- Preferences › Advanced › Database backs the library up on request, lists
  the backups, and puts one back; and finds duplicates, tracks that share a
  title and an artist, with a copy removed at a time.
- Loops are drawn on both waveforms, and a memory loop called plays as a
  loop.
- An export says which track it is on, and a stick plugged in or pulled out
  is noticed within two seconds.
- The BPM is typed over in the list and the Info tab, and the beat grid is
  retimed to it so the CDJ agrees with the column.
- Add Artwork and Delete Artwork on the Info tab's Artwork page.
- PHRASE EDIT on the GRID panel: CUT splits the phrase under the head and
  CLEAR takes it out.
- My Tag is edited on the Info tab: every tag of the library a toggle.
- Related Tracks, a section of its own on the rail: BPM + KEY (within six
  percent of the track on Player 1 and in its key or one beside it on the
  wheel), Same genre in 30 days, and Same artist. With no track loaded the
  criteria open empty, as rekordbox's do. Rust picks the rows.
- The keyboard map is rekordbox's Export preset, built out: Loop In, Loop
  Out and Exit/Reloop on I, O and R, the beat loops on 4 to 9, / and
  option + \\ for the length, Memory Cue 1 to 10 on A to ;, SYNC, MASTER
  TEMPO, Tempo Reset and BPM ± on the function keys, F9 for the metronome's
  sound, and the master's volume and Mute on command + F10 to F12. Player
  B's keys are Player A's with shift. Every key is changed from the
  Keyboard pane: click its badge, press the new one; Backspace takes it
  away, Reset puts the preset back.
- Preferences › PRO DJ LINK is a pane of its own.
- Preferences › Advanced › Update Manager asks how often to check: every
  start, once a day, once a week.
- The About pane: the version, the licence, the disclaimer, and links to
  Instagram, the web and Twitch.
- Preferences › Audio › Master limiter meters the output per channel and
  the limiter's reduction, each with its decibels.
- A track played for a minute goes on today's history session and its DJ
  Play Count goes up, as rekordbox records it; Remove from History takes a
  play off again. Preferences › Advanced › Browse turns the recording off.

### Fixed
- A cloud-synced track whose file is not where the library says is exported
  from its local copy rather than skipped.
- The LINK strip stays hidden while nothing is on the network, even when
  LINK cannot be turned on, and the Connect button keeps its space.
- A click that selects a row no longer opens the cell's editor as well.
- A stick's DEVSETTING.DAT is written when the device panel opens on it, as
  rekordbox does, rather than on every export.

## [0.7.0] — 2026-09-16

### Added
- Pro DJ Link (EXPORT). Turn LINK on and the app appears to the CDJs and
  mixer on the network as a rekordbox source: players browse the library
  and load tracks — artwork, waveforms, cues, key and BPM — and play the
  audio itself, exactly as they do from rekordbox. Nothing is written to the
  library.
- The LINK strip along the bottom shows every player and the mixer on the
  network, what each has loaded from the app, and its CUE / PLAY / MASTER /
  SYNC state. Drag a track from the library onto a player to load it there.
- Tempo master. The app can be the network's tempo master: set the BPM with
  the −/+ buttons or take it from whichever player is master, then press
  MASTER, and every player set to SYNC follows the app's tempo and downbeat.
- DJ System › PRO DJ LINK settings. Turn LINK on and off, see the players on
  the network, and choose which network interface LINK runs on — Automatic
  (the one the players are reached through) or a specific one by name.
- The arrow keys move the highlighted track up and down the list; Enter loads
  the highlighted track onto Player 1, and Left / Right beat-jump it.
- Drag a playlist or folder to a new place in the tree — between two others,
  or into a folder — and it stays there in rekordbox too. Rename Playlist and
  Rename Folder in the tree's right-click menu type the name over in place.
- Drag tracks within a playlist to reorder it. The drag is offered only while
  the playlist is shown in its own order — not sorted by a column, searched
  or filtered.
- Artist, Album, Genre and Label are edited in the list: double-click the
  cell (or click it on a selected row, with Edit Library › Double-click to
  edit off), type, and press Enter; Escape abandons.
- The status bar shows the version beside the app's name, and Preferences ›
  About says who the app is by.
- The device panel warns that USB export is still in development before any
  tab is opened.

### Changed
- The master starts a decibel under full, and the knob reads 0 to 10 while
  it turns, 10 being that decibel of headroom; pulled past 10 it holds,
  then lets go to 11 at six o'clock, full level.
- The metronome's click keeps its own volume whatever the master is at.
- The beat count at the top of the waveform is bars.beats: the second
  number runs 1 to 4.
- A click on the enlarged waveform plays a stopped deck; a second click
  pauses it and sets the cue at the head. The switch is View › "Click on
  the waveform for PLAY and CUE".
- Analyze Track is shift + command + A: the preset's A calls Memory Cue 1.
- The 3Band waveform is painted in rekordbox's own seven colours, one per
  combination of bands that reach a pixel: blue for the low alone, brown
  where the mid overlaps it, the cream core where all three do, amber and
  white for a mid or high standing alone, and the two rarer pairs. The
  same palette for the overview and the detail.
- The tree scrolls to its last row and no further: scrolled to the bottom,
  the last playlist sits at the bottom of the pane rather than a pane's
  height above it.
- The detail waveform is a little shorter in its band, and the beat
  markers reach 15px above and below it.
- The read-only badge is blue, and says why on hover; the LINK button is
  not shown until LINK can be turned on; the blurbs are gone from every
  Preferences pane, and so is the blue halo on a secondary window.
- The Preferences window has the app's own title bar on every OS, and its
  Keyboard pane opens with every group closed.
- DJ System › PRO DJ LINK reads "Connect to PRO DJ LINK" / "Disconnect", with
  the network interface on its own row.
- Cloud Library Sync and Auto Upload are gone from the right-click menus
  rather than greyed — there is no cloud library behind the app. Analyze
  Track is greyed everywhere until analysis is worth offering.
- Scrolling the list quickly no longer shows blank rows: the rows either side
  of the screen are fetched ahead, a row still loading draws a placeholder,
  and waveforms start loading before their row scrolls into view.

### Fixed
- No stray blue border appears around the Preferences or Update window.
- The LINK button says why it cannot turn on — usually rekordbox already
  running and holding the network ports — instead of doing nothing.
- The CPU figure in the title bar shows the app's real load. It read 0%
  whatever the app was doing, and now keeps a decimal below 10% so an idle
  app reads as 0.1% rather than 0%.
- The window reopens where it was left — on a second display, say — even
  after a crash or a force-quit. Its position was saved only on a clean quit.

## [0.6.0] — 2026-09-11

### Fixed
- A rating set in the app is stored as the number of stars, which is how
  rekordbox stores it. Ratings were written on the wrong scale, so a
  four-star track showed five stars in the browser, and the information
  panel read every rating as none.
- The Histories section is there on every start. It was missing whenever
  the library came from the app's own snapshot, which is most starts.
- A folder that has nothing in it yet is drawn and treated as a folder: a
  playlist made from its menu goes inside it, and its menu offers Delete
  Folder.
- Create New Playlist and Create New Folder work from a playlist's menu. They
  did nothing but show an error, because the tree asked for a parent the
  library does not know.
- Dragging a selection onto a playlist adds every selected track, not only
  the one under the hand; dropped on a deck, the first of the selection
  loads, as in rekordbox.
- Right-clicking a selected row keeps the selection, so Remove from
  Playlist takes every track that was chosen.
- A rating or comment set after a playlist edit stays set once the library
  has been re-read, instead of lighting for a moment and going out.

### Changed
- The library is backed up before the first edit of a session, not before
  every edit. Each rating click and each drop on a playlist copied the whole
  database.

## [0.5.3] — 2026-09-10

### Fixed
- On Windows, the Preferences window opens with its contents instead of a
  blank white frame, without the application menu bar on it, and closing the
  main window with Preferences open quits the app rather than leaving it
  running with only Preferences.
- On Windows, the menu's keyboard shortcuts — Ctrl+, for Preferences,
  Ctrl+O, Ctrl+I, Ctrl+B, Ctrl+7 to Ctrl+0 for the layouts — work while the
  app has keyboard focus.
- A window that could not open the library shows an empty list, not the
  previous session's tracks.
- The app idles within its processor budget: the cost readout in the title
  bar cost more than the budget it reports, and now reads every five seconds
  and re-renders only itself.

## [0.5.2] — 2026-09-10

Everything in 0.5.0 and 0.5.1, neither of which published — 0.5.0's macOS
build made a disk image but no update the app could take, and 0.5.1 built on
both platforms but its publish step deleted its own installers before
uploading them. This is the release that ships the self-updating app.

### Fixed
- Escape no longer closes the Update Manager while a download or install
  is running.

## [0.5.1] — 2026-09-10 — not published

## [0.5.0] — 2026-09-10 — not published

### Added
- The app keeps itself up to date. Check for Updates… in the application
  menu, or Preferences › Advanced › Others, opens the Update Manager: it
  shows the version running and the latest one, what changed between them,
  and downloads the new version with a progress bar, installs it and
  restarts. A check runs on its own shortly after launch and only opens the
  window when there is something new; that can be switched off in the same
  Preferences pane. Every download is checked against a signing key built
  into the app before it is installed.
- On Windows the update installs behind a small progress window rather
  than the full installer.

## [0.4.0] — 2026-09-10

Everything tagged as 0.3.0 plus the work below. 0.3.0's build was cancelled
before it published — its tag missed fourteen commits that had not reached
the remote — so this is the release that ships both.

### Added
- The settings gear opens rekordbox's Preferences — View, Audio, Analysis,
  DJ System, Keyboard and Advanced — as a window of its own that can be
  moved, and its choices change the app and are kept between runs. The master
  limiter lives in Audio.
- BEAT SYNC holds a deck to the master's tempo until RST or MASTER ends it;
  it can take a double or half BPM as a match, or match the tempo alone. With
  Q on, play starts a deck on the master's beat, and a synced deck waits for
  the master's next beat before it sounds.
- A quantized cue can snap to a half, quarter or eighth of a beat.
- The Traffic Light lights the keys that go with the loaded track's, and the
  # column sorts a playlist by its own order.
- The 2 PLAYER details are half waveforms that meet at the line between the
  decks.
- A fresh stick takes the DJ System defaults on export; missing tracks can be
  relocated from search folders; an import says which tracks landed.

### Fixed
- A track dragged to a player carries a faded copy of its row, every time,
  in the shell as well as the browser; the sleeve no longer gets a dashed
  border.
- Dragging the waveform of a freshly loaded track sounds right without
  pressing play first.

## [0.3.0] — 2026-09-10 — not published

The build was cancelled before it reached the bucket; see 0.4.0.

### Added
- A master limiter on the mix bus, so two decks at full level no longer
  distort. Settings › Audio output has the switch, a ceiling (−12…0 dBFS,
  −0.3 by default) and a release (10…1000 ms, 100 by default), and shows how
  far the sum is being turned down. The setting is remembered between
  sessions.
- The 2 PLAYER layout is the one rekordbox draws — two full decks meeting in
  the middle — rather than the single deck at half height.
- Every empty sleeve shows the record the track list draws.
- A hot cue on the detail waveform is its lettered square in its colour, under
  the memory cue's red triangle, the way rekordbox draws it.
- The right-click menus in the tree and the track list list what rekordbox's
  do, in its order, at its size and colours.
- The MEMORY list draws ten boxes, so a track with no memory cues shows an
  empty grid rather than nothing.
- The icon column beside the browser offers Information and Sub-Browser only.
- The top bar no longer shows the info button or the Professional badge.

### Fixed
- A track dragged onto a deck loads in the real app, not only in the browser,
  and the simple player's sleeve shows it will take one.
- Dragging a track lights only the playlist under the pointer, not every
  playlist in the tree.
- The beat grid's heads sit at their measured height with the line under
  them.
- The title bar, status bar, search field, scrollbars, icon column and the
  rows above the list are the greys, faces and sizes rekordbox's window has.
- The track list's horizontal scrollbar sits at the panel's foot, not under
  the last row.
- A track without artwork shows rekordbox's record in the list, not a
  coloured tint.

## [0.2.0] — 2026-09-09

### Added
- The deck's INFO tab shows the loaded track's rating, colour, comment and
  file the way rekordbox's manual lists them.
- Hot cues: set from an empty pad, called from a set one, cleared from the
  HOT CUE list; the browser row's CUE mark follows the edit without a reload.
- Memory cues are set, called and deleted from the deck; M, B, N and X are
  bound as rekordbox binds them. Memory cues, hot cues and loops can be added,
  moved and deleted.
- Hot cues show on every waveform in the colours rekordbox paints them.
- The information panel has rekordbox's Summary, Info and Artwork tabs, and
  the Info tab edits what the writer can safely take.
- An Explorer section in the tree opens the disk's folders as track lists; a
  folder with more subfolders than the tree can show says how many were left
  out.
- A selected device opens the six settings tabs rekordbox draws for it, and a
  stick's settings can be read and written back.
- The Track Filter drops down from the browser header and narrows the list by
  BPM, key, rating and colour, all in Rust.
- The sub-browser is a second browser beside the first, opened from the icon
  column, with its own selection.
- The simple player is one strip, the way rekordbox draws it.

### Changed
- Reading a file's tags no longer reads its cover art; a page of loose
  Explorer files reads its tags eight at a time, within a budget.
- End-to-end tests run at the capture's 1800×1130 rather than a device
  preset.

### Fixed
- Cue markers and the HOT CUE list no longer vanish on a start that hits the
  snapshot.
- A loop in the memory list is labelled as rekordbox labels a cue.
- A waveform whose element is swapped by a layout switch is drawn at its new
  size.

## [0.1.0] — 2026-09-09

The first tagged build: an export-mode rekordbox clone that reads the real
library, plays it through its own engine, analyses tracks, serves CDJs over
Pro DJ Link and writes USB exports. Everything below landed between
2026-09-07 and the tag.

### Library
- Reads the real rekordbox `master.db` read-only, within every performance
  budget; starts from a cached library (666 ms down to 75 ms) and draws the
  last screen before the library is read.
- Rust owns sorting, filtering and search; the list fills the window, columns
  can be chosen, reordered and resized, and each kind of table remembers its
  own; sort goes ascending, descending, then off; rows are numbered.
- Playlists can be created, renamed, moved and deleted; tracks dragged onto
  them; ratings, comments and colours edited in the list. Every write is a
  soft delete with a USN bump in one transaction, refused while rekordbox is
  running, with a backup taken before the first write of a session.
- Tracks whose files have gone are found and can be relocated; music files
  can be added to the library; a selection can be analysed with progress that
  can be stopped.
- The Histories section shows the sessions rekordbox recorded and what was
  played in each.
- Artwork and the three-band waveforms are drawn in the browser through the
  `rbl://` scheme; a missing sleeve shows a cached one or an empty square, not
  a broken-image mark.

### Player
- A real audio engine: two decks, one clock, play/pause/seek without clicks,
  a smooth playhead, and a drag-scrub that sounds like a record — slow drags
  turn it evenly, a fast drag plays a burst and stops.
- A deck can be played faster or slower with MASTER TEMPO holding the pitch
  accurately enough to shift a key; beat sync pulls one deck onto the other's
  tempo and bar.
- A channel strip per deck — trim, three bands, kills — and a crossfader; a
  master level whose moves do not crackle, with meters that fall in time.
- The deck rebuilt as the one rekordbox shows: measured geometry and palette,
  CDJ controls, pad and panel tabs that switch, the beat grid, cue markers,
  the bar count, and hot cues drawn in their colours on both waveforms.
- 1 PLAYER, SIMPLE PLAYER and 2 PLAYER layouts; a playing deck carries on
  through a switch. Deck B reads bottom-up so the two waveforms meet.
- A track reaches a deck three ways: dropped on it, from the menu, or by
  clicking an empty one.
- Audio can be sent to any output the machine has.
- rekordbox's own keyboard shortcuts and right-click menus.

### Analysis and devices
- Tracks are analysed for tempo, key and waveforms; analysis files reproduce
  rekordbox's byte for byte; tempo matches rekordbox's value on 95% of tracks
  and to a hundredth of a BPM; key agrees on far more tracks; phrases and
  vocal placement are read and drawn.
- USB exports a CDJ can browse, with every table type rekordbox writes, and
  only what changed copied on a second export to the same stick; rekordbox
  reads a stick this app exported.
- The app announces itself on a Pro DJ Link network, shows which players and
  mixers are on it, serves the remote-database protocol CDJs browse over and
  lets a player mount the library over NFS.

### App
- Native macOS menu bar, the app's own icon, a Content-Security-Policy, and
  the window reopens where it was left — on the screen, not half off it.
- The interface stays responsive while artwork and audio load; the window no
  longer re-renders six hundred times a second.
- Signed builds for macOS (opens without Gatekeeper refusing it) and Windows,
  published with readable download URLs.

[0.5.3]: https://github.com/chrisle/rbxport/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/chrisle/rbxport/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/chrisle/rbxport/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/chrisle/rbxport/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/chrisle/rbxport/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/chrisle/rbxport/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/chrisle/rbxport/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/chrisle/rbxport/releases/tag/v0.1.0
