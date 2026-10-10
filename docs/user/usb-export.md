# USB export preferences

[Documentation](../README.md) · [USB format reference](../reference/usb-export-db.md)

USB export writes player libraries and audio copies to a device. These
preferences apply to playlist export and Sync Manager; compatibility
conversion also applies to individual track export.

## What a selection exports

- A playlist exports its tracks. An intelligent playlist exports the tracks
  its rule admits at the time of export.
- **Export Folder** exports every playlist and intelligent playlist under the
  folder, at any depth. The device gets the folder, its subfolders (including
  empty ones), and the folders above it, with each playlist inside its folder.
- **Export Playlist** and **Export Folder** (right-click a playlist or folder,
  then choose a connected stick) add to what the stick already holds, as
  rekordbox's do: playlists exported earlier, by either menu or Sync Manager,
  stay on the stick, and the stick's own playlists are kept. Exporting a
  playlist the stick already holds updates it in place.
- Sync Manager makes each ticked stick hold exactly the ticked playlists: a
  playlist from your library that is no longer ticked is taken off the stick.
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
the top, as in rekordbox. When a playlist lists a track twice, selecting
either copy selects both, and Remove from Playlist takes both out (rekordbox
selects and removes each row on its own).

Each change is written to the library it was made in and not the other, as
rekordbox does. Deleting a playlist leaves its tracks on the stick. These
edits are refused while rekordbox is running, and greyed while the stick is
being synced, exported or ejected. Adding tracks from the collection to a
stick's playlist is an export: use Export Playlist or Sync Manager.

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
Cleanup runs after the new export is published. Empty playlist selections
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

Conversion changes export copies, preserving source audio and analysis timing.
It updates device database/analysis paths and reuses unchanged conversions on
later syncs. Changing format replaces the previous exported copy.

Conversion errors stop publication. Surround audio is refused instead of
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
