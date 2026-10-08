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
- Sync Manager lists playlists, intelligent playlists, and folders. Ticking a
  folder ticks every playlist and intelligent playlist under it. The list
  updates when the library changes while the window is open; ticks on
  deleted playlists are removed.

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
