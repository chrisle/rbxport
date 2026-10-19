# USB export preferences

[Documentation](../README.md) · [USB format reference](../reference/usb-export-db.md)

USB export writes player libraries and audio copies to a device. These
preferences apply to playlist export and Sync Manager; compatibility
conversion also applies to individual track export.

## Import from a USB device

Sync Manager has two actions. **SYNC** writes the ticked playlists to the
ticked USB devices and never brings anything back. **Import** brings back
from the ticked USB devices what is turned on under **Preferences → USB
Export**, with no further questions:

| Preference | Default | What Import does |
| --- | --- | --- |
| Import cues and beat grids | On | Replaces cues and beat grids for matching tracks in your library with the device's. |
| Import play history | On | Adds the device's new play-history entries to your library. |
| Import CDJ/mixer settings | Off | Keeps the device's CDJ and mixer settings in RBXport; SYNC gives them to devices that have no settings of their own. |
| Import track ratings | On | Brings back ratings changed on the device since its last sync. |

The settings button beside **Import** (**Change import preferences**) opens
that Preferences pane at its Import section, and the one beside **SYNC**
(**Change sync preferences**) opens it at its Sync section, which holds
everything SYNC does: setting up the PIONEER folder on a new drive, deleting
music not in any playlist, Maximum CDJ compatibility and Verify all files. Library Protection must be off to import cues, play
history or ratings, and rekordbox must be closed to import play history or
ratings; a kind that cannot be imported for either reason is left out and the
status line says why.

This matches rekordbox, which brings device edits back into its collection
only when you ask (**Update Collection** on the device, or the device-to-
rekordbox arrow in its Sync Manager), not as part of a sync.

SYNC goes from your library to the device, so for the tracks it writes your
library's cues and beat grids win: a cue or beat grid changed on a player is
replaced by your library's. To keep a player's cues and grids, run **Import**
before SYNC. If your library has no analysis for a track, SYNC keeps the
device's waveforms and beat grid for it, edits included.

A cue a player added to a track that SYNC would take off the device would
just be lost, so SYNC stops before copying anything and names the track.
Keep a playlist that holds the track in the sync, or delete the track from
the device in rekordbox, then sync again. Importing the cue does not clear
this stop.

A rating changed on the device stops SYNC for that device until it is
imported ("changed on the USB. Import its changes before syncing."), so a
sync overwrites neither the device edit nor your library.

### Track ratings

Changes in either OneLibrary or legacy Device Library are imported, including
clearing a rating to zero. Unchanged device ratings preserve desktop edits.
When both the device and desktop rating changed, the device wins and the
result reports how many desktop edits were replaced. Repeated imports of
already matching ratings make no further changes.

Conflicting edits in the two device databases stop the import; reconcile
them in rekordbox first. Devices without a matching RBXport export baseline
are skipped with a warning; sync the device once to create one. Normal
exports write 0–5 stars to both device formats.

RBXport 1.6.0 and earlier wrote OneLibrary ratings on rekordbox XML's scale
(51 per star). Players that read OneLibrary, such as the CDJ-3000X, show no
stars for those tracks. Sync the USB once to rewrite them as 0–5 stars;
rating import reads the old values as the same stars.

## What a selection exports

- A playlist exports its tracks. An intelligent playlist exports the tracks
  its rule admits at the time of export.
- Each track goes on the stick with the BPM, key, length and analysis files
  your library holds for it when the export starts, including a track
  analysed while an analysis run is still going.
- **Export Folder** exports every playlist and intelligent playlist under the
  folder, at any depth. The device gets the folder, its subfolders (including
  empty ones), and the folders above it, with each playlist inside its folder.
- **Export Playlist** and **Export Folder** (right-click a playlist or folder,
  then choose a connected stick) add to what the stick already holds, as
  rekordbox's do, and so does **Export Track**: playlists and folders exported
  earlier, by either menu or Sync Manager, stay on the stick in their place,
  including empty folders and playlists since deleted from your library, and
  the stick's own playlists are kept. A playlist still in your library is
  updated to what it holds now; one deleted from your library stays as the
  stick has it.
- Dragging does the same as those menus, as in rekordbox: drop tracks on a
  stick's row under **Devices** (or on its Device Library, OneLibrary or All
  Tracks) to export just those tracks, as **Export Track** does, and drop a
  playlist or folder on the stick's row to export it, as **Export Playlist**
  and **Export Folder** do. The tree shows one section at a time, so open the
  Sub-Browser to have your playlists beside the stick. Tracks dropped on one
  of the stick's own playlists are not exported there.
- Sync Manager makes each ticked stick hold exactly the ticked playlists: a
  playlist from your library that is no longer ticked, or that was deleted
  from your library, is taken off the stick.
- Sync Manager lists playlists, intelligent playlists, and folders. Ticking a
  folder ticks every playlist and intelligent playlist under it. The list
  updates when the library changes while the window is open; ticks on
  deleted playlists are removed.

## Playlists on a stick

Open a stick under **Devices** to see the libraries it holds, **Device
Library** and **OneLibrary**, each with **All Tracks** and its **Playlists**,
as rekordbox lists them. Select a playlist to see its tracks.

Right-click the Playlists heading or a folder to create a playlist or folder
on the stick; right-click a playlist or folder to delete it. To rename one,
click it again once it is selected, as in rekordbox. Over the stick's
tracks, **Add To Playlist** lists that library's playlists, and inside a
playlist **Remove from Playlist** takes tracks out. A new playlist goes at
the top, as in rekordbox. When some of the tracks are already in the
playlist, rbxport asks first, as rekordbox does: **Add duplicates** puts them
on the end again, **Skip duplicates** adds only the others. The choice in
Preferences > Advanced > Playlists applies here too. When a playlist lists a track twice, selecting
either copy selects both, and Remove from Playlist takes both out (rekordbox
selects and removes each row on its own).

Each change is written to the library it was made in and not the other, as
rekordbox does. Deleting a playlist leaves its tracks on the stick. These
edits are refused while rekordbox is running, and greyed while the stick is
being synced, exported or ejected. Adding tracks from the collection to a
stick's playlist is an export: use Export Playlist or Sync Manager.

### Import a stick's playlist into the collection

Right-click a playlist on the stick and choose **Import Playlist** to copy it
into your collection, as rekordbox does. This is how a playlist made on a
player gets back to your library, such as a TAG LIST an XDJ-AZ saved as a
playlist. The new playlist goes at the end of the top level of your
playlists, with the stick's name and track order. If a playlist or folder
there already has that name, the new one is numbered (`Name (1)`,
`Name (2)`, ...) and a note says so. A track listed twice is imported twice.

A stick track is matched to the collection track at the same file, or to
the collection track it was exported from. What happens to a track your
collection does not have is set in **Preferences > Advanced > Browse >
When importing a playlist from a device**:

- **Copy missing tracks** (the default): its file is copied to
  `PioneerDJ/Imported from Device` in your Music folder, under
  `Contents/<Artist>/<Album>/`, added to your collection, and imported with
  the rest, as rekordbox does when you click OK. A file already there is
  never replaced: the copy gets a numbered name (`Track-1.mp3`). Each copy
  is checked against the stick's file before it is used. If the import
  fails, the copies it made are removed again. Importing the same playlist
  again uses the earlier copies instead of copying the files a second time.
- **Ask each import**: rekordbox's question, with how many files would be
  copied, their size and where they go. OK copies, Cancel imports nothing.
- **Import only already-matched tracks**: the track is left out and the note
  says how many were. If none of the tracks are in the collection, nothing
  is imported.
- **Cancel when any track is missing**: nothing is imported, and the note
  says how many tracks are missing.

A copied track is imported from its file's tags. The stick is only read.

## Music already on the device

As in rekordbox, a track whose file the library already keeps on the
destination device is not copied. The device databases name the file where it
is, and export writes only the databases and the track's analysis. A path
with a component the device cannot carry (a reserved character, or a name
ending in a dot or a space) is copied under `Contents/` instead, as rekordbox
does. Export never deletes or replaces such a file, including when the track
later leaves the selection or music cleanup is enabled.

## Delete music outside playlists

Off by default. When enabled, export removes RBXport-exported audio outside
all selected playlists, including previously exported individual tracks.
Owned-file cleanup is included in the recoverable export. Empty playlist selections
are refused; original library audio and unrelated device files are preserved.

## Maximum CDJ compatibility

Off by default. When enabled, incompatible mono/stereo audio, including FLAC
and ALAC, is converted to the selected output format:

| Format | Output |
| --- | --- |
| WAV (default) | 16-bit PCM, 44.1 kHz, stereo; resamples and reduces higher-resolution input. |
| AIFF | 16-bit big-endian PCM, 44.1 kHz, stereo; lossless with the same audio data as WAV. |
| MP3 | 320 kbps CBR, 44.1 kHz, stereo; smaller files with lossy compression. |

Compatible MP3 and integer PCM WAV/AIFF at 44.1 or 48 kHz are copied as-is.
Selecting MP3 does not recompress a compatible WAV or MP3. This targets older
USB-capable players; filesystem and model-specific library requirements
still apply. The [CDJ-350 specifications](https://www.pioneerdj.com/en/product/player/cdj-350/)
are one format reference, not proof of every player's compatibility.

## Conversion and incremental sync

When playlists or export preferences change, Sync Manager updates each USB's
space bar with projected additional use and space left. The separate temporary
space line includes the room needed to keep the previous library recoverable.
Conversion estimates are approximate until a completed cached copy is available.
An identical sync says “Already up to date”; a playlist-only edit leaves
unchanged audio, analysis, and artwork alone. Several sticks can share one
conversion while retaining independent progress and results.

Progress identifies checking, conversion, copying, database work, verification,
and saving. Stop works before publication; “Stop all exports” stops cancellable
jobs together. Saving must finish before eject. An interrupted export is
recovered on the next explicit operation. Do not unplug while saving.

Missing sources keep a usable existing USB version and report a warning. Tracks
without a usable copy are skipped. If audio changed but its analysis did not,
re-analyse it before expecting current waveforms/cues. **Verify all files
(slower)** in Preferences › USB Export › Sync reads exported audio and analysis to detect changes that preserve
size and timestamps; ordinary sync uses inexpensive verified receipts.

Conversion changes export copies, preserving source audio and analysis timing.
It updates device database/analysis paths and reuses unchanged conversions on
later syncs. Changing format replaces the previous exported copy.

Conversion errors retain usable older device copies or skip new failed tracks,
with warnings. Publication/storage failures preserve the previous generation
or leave a recovery journal. Surround audio is refused instead of
dropping channels; WAV and AIFF output beyond their 32-bit container limits is
refused.
The app uses Symphonia, Rubato, built-in PCM WAV/AIFF writing, and bundled LAME. Running
export does not require a separate FFmpeg installation.

## Code and tests

`crates/rbl-export/` owns export and publication, working with `rbl-pdb`,
`rbl-onelibrary`, `rbl-anlz`, and device integration. The format reference
explains the full file set and verification boundaries.

The stereo FLAC fixture contains generated 440 Hz left / 880 Hz right tones
at 96 kHz. Tests cover stereo, duration, resampling alignment, MP3 bitrate,
incremental reuse, format changes, source preservation, and failure before
publication. FFmpeg generated that fixture; tests do not require it.
Player behavior needs separate firmware/physical validation; see [Testing](../development/testing.md).
