# Actual stock rekordbox Cbm capture

[OBS] Captured 2026-11-08 on chris-m2-mac from signed, unmodified
rekordbox 6.8.7.0303, executable SHA-256
`aab4838b9768860da941f232db3f31cfa397a7a79f6052cde1aa6584f2d5dc3c`.
The user authorized the local M2 test Collection and disposable TRIODE2.
Cloud Library Sync stayed off. Exported only two generated stereo 16-bit
44.1 kHz WAVs, selected DJ System > Device > XDJ-R1, then completed the
stock Create library operation. These six DBH files are actual stock output,
not generated RBXport fixtures. `sha256.json` preserves their original hashes.

Classic PDB and WAVs were independently used by `xdj_r1_compare_pdb` to
construct the matching RBXport input. Content IDs: beta=1, alpha=2; playlist
ID=1, name `R1 Comparison Test 20261108`, order beta then alpha. Both tracks
have BPM 150, Auto Load on, no artwork or named artist/album/genre/key.
The tests compare every byte except each file's timestamp at 0x10..0x13.
Stock generated timestamps can differ between category files.

Source alpha.wav (3 seconds, data offset 78, 529278 bytes) SHA-256:
`68f835294ca290a37d0daba61bcaa463867c660c65b83d1d6e68f3dd7d9ae78f`.
Source beta.wav (4 seconds, data offset 78, 705678 bytes) SHA-256:
`4c71fb2d1543ccff053910667375e20c2d34ccb2cb6f8e214e0483760ca95ff1`.
Prepared WAV hashes matched their inputs. FAT32 device blocks were 512 bytes.
The complete private capture/tree is retained under
`~/r1-research/captures/20261108-triode2/stock-after-eject`.

[OBS] Header +0x04 in all five categories is 0xb04c, the CbmDB content-table
end. The initial RBXport writer left it zero; this capture found that gap.
[UNKNOWN] This small PCM/category fixture does not establish physical R1
compatibility, artwork/cue recall, other formats, text boundaries, pending
edits, or interrupted stock export behavior. No hardware player was tested.
TRIODE2 must remain mounted or be unmounted only; NEVER eject it again.
