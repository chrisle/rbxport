# LINK behavior and hardware coverage

[Documentation](../README.md) · [General test guide](../development/testing.md)

## Scope

This reference covers RBX as a rekordbox Link Export source: firmware browsing,
loading, and interaction over the real protocol. The separate XDJ-AZ USB gate
covers a generated export and deck-1 playback. Mixer controls, effects,
recording, streaming services, and broader USB compatibility are outside this
LINK behavior catalog.

Use temporary fixtures for writes and set `RB_LITE_TEST=1` for public Rust
checks. Booted firmware, protocol-client, and physical-device results must be
reported as separate evidence layers.

## Evidence rules

- `[OBS]` is backed by a product manual, a packet capture, or identified firmware behavior.
- `[ASSUME]` is a compatible interpretation that still needs a capture or physical-device check.
- `[UNKNOWN]` means neither static evidence nor the emulator can establish the result.

AtEmu proves the response, transport, and firmware behavior that its device model implements. It still cannot prove physical controls, audio output, or behavior omitted by the model. A catalog row is complete only for a model whose vendor firmware actually booted during the test; substituting a device name in a mock or protocol client does not count.

## Source baseline

| Device | User manual | Firmware reference | AtEmu status | Important compatibility detail |
| --- | --- | --- | --- | --- |
| XDJ-AZ | XDJ-AZ instruction manual | Firmware 1.30 | **Passing emulated USB gate:** generated FAT32 export browses, loads on deck 1, and passes tone/cue audio tests. | One appliance exposes two ordinary Pro DJ Link deck identities; four-deck status can use USB slot `07`. Physical XDJ-AZ behavior is not established by this gate. |
| XDJ-RX3 | XDJ-RX3 instruction manual | Firmware 1.20; activation capture on 1.19 | **Unavailable:** no bootable emulator model. | Uses legacy request variants for several browse/load operations. Rear USB Link Export does not start until the audio gadget reports its connection event. Physical check on Linux, firmware 1.20: source, browse, load, play and auto-join pass. |
| CDJ-3000 | CDJ-3000 instruction manual | Firmware 3.20 | **Available:** vendor firmware boots and supports automated panel interaction. | Uses live keyboard search `1500`; after a Link Export load it waits for user-info `3006` before requesting delivery info `2602`. |

The XDJ-AZ and XDJ-RX3 manual actions are in **Track selection**; the CDJ-3000 equivalents are in **Track selection** and **Browsing tracks**. Packet expectations are based on firmware analysis and protocol captures, with evidence markers distinguishing observations from assumptions.

### XDJ-RX3 on Linux

The rear-USB activation lease (`0x50` over USB-MIDI) also runs on Linux, through ALSA (`midir`). The port shows as `XDJ-RX3:XDJ-RX3 MIDI 1 <client>:0`.

The RX3's USB NIC (`2b73:0007`, `cdc_ether`) uses IPv4 link-local (169.254/16). Windows and macOS self-assign it when DHCP gets no answer. NetworkManager's default wired profile is DHCP only, so the interface gets no IPv4 and RBX reports "no 169.254.x.x address". Bind a profile to the NIC:

```sh
nmcli con add type ethernet con-name rx3-link ifname '*' \
  802-3-ethernet.mac-address <RX3 NIC MAC> \
  ipv4.method link-local ipv6.method link-local connection.autoconnect yes
```

On NetworkManager 1.52 or later, `ipv4.link-local fallback` (DHCP first, then link-local) also works.

## Test layers

### Unit tests: RBX code with mocks

Unit tests should be deterministic and fast. The main Link Export suite is `crates/rbl-dbserver/tests/session.rs`; its mocked `Catalog` provides exact rows and blobs. Add a unit test when changing any of these contracts:

- request dispatch and model-specific opcode aliases;
- root, category, sort, search, playlist, history, and track queries;
- stable IDs, filtering, sorting, playlist order, history order, and played state;
- menu headers, pagination, rows, footers, and item-position replies;
- metadata, artwork, waveform, beat-grid, cue, and named analysis reply envelopes;
- setup, user-info, delivery-info, empty, error, and unsupported-request replies;
- edit validation, persistence, cache invalidation, and read-only refusal.

Use complete encoded-message comparisons for protocol layouts. Use semantic assertions for catalog queries and persistent state. A unit test should fail at the smallest responsible component and should not duplicate a socket integration test merely to increase count.

### Protocol integration tests

`crates/rbl-link/tests/link.rs` builds a temporary library, starts RBX services on loopback, and drives them with `rbl-fakecdj`. These tests cover socket framing, RemoteDB, portmap, mountd, NFS, and model-specific opcode handling. They do not start AtEmu and must never be reported as hardware-emulation results.

| Operation | XDJ-AZ | XDJ-RX3 | CDJ-3000 |
| --- | --- | --- | --- |
| All tracks | `1004` | `1200` content-tracks path | `1004` |
| Key browser | `1014` related-key family | `100b` legacy key family | `1014` related-key family |
| Keyboard search | `1300` until a device capture establishes otherwise `[ASSUME]` | `1300` | `1500` `[OBS]` |
| Artwork while loading | `2003` | `2103` content-artwork path | `2003` |

### AtEmu firmware integration tests

The CDJ-3000 firmware suite uses an isolated three-track library and drives the stock firmware through its panel controls. Results include screenshots, app logs, emulator health, and test reports. The firmware harness and its artifacts are maintained separately from this public repository.

An AtEmu case must assert a user-visible result, not only that a packet was accepted. Load coverage continues through firmware browsing, path lookup, NFS read, analysis retrieval, playback motion, and RBX receiving the deck's status packets.

## Hardware behavior catalog

“Expected on screen” comes from the manuals. “Automated oracle” is what RBX can prove without the physical display. All rows apply to all three devices unless the device column narrows them.

| ID | Device | User action | Expected on screen or device | Automated oracle | Evidence / status |
| --- | --- | --- | --- | --- | --- |
| HW-NET-01 | All | Connect the player and RBX to the same Link network and start RBX. | RBX becomes an available rekordbox source; browsing does not begin before Link is up. | A compatible keep-alive moves RBX from `Waiting` to `Up`; the RemoteDB port query returns the active database port. | Manuals: PRO DJ LINK / rekordbox Link Export. `[OBS]` |
| HW-SRC-01 | All | Open SOURCE and select the RBX rekordbox source. | The browse screen opens with configured categories. | `1000` returns the root menu and the emulator renders all rows through header/items/footer. | Manuals: Selecting a source. `[OBS]` |
| HW-BRW-01 | All | Select TRACK with the rotary selector or touchscreen. | A track list is shown with title and configured secondary fields. | The model's all-tracks request returns all fixture tracks; the first expected title has a stable track ID. | Manuals: Selecting a track; Browse screen. `[OBS]` |
| HW-BRW-02 | All | Select PLAYLIST, select a playlist, then open it. | Playlist folders/lists appear, followed by the playlist's tracks in playlist order. | `1105` folder mode returns the fixture playlist; list mode returns two members and first position `1`. | Manuals: playlist screen and selecting a track. `[OBS]` |
| HW-BRW-03 | All | Select HISTORY, then select the displayed history with the rotary selector. | A named history is shown; selecting it shows the tracks recorded in that history. | `1012` returns one history and `1112` returns a non-empty history track list with played-row semantics. | Manuals: Using History. `[OBS]` |
| HW-BRW-04 | All | Open SEARCH and enter `AT`. | Tracks containing the keyword are shown. | RX3/AZ `1300` or CDJ-3000 `1500` returns a non-zero result count. | Manuals: Searching for a track; CDJ-3000 opcode from firmware/capture. AZ opcode `[ASSUME]`. |
| HW-BRW-05 | All | Open the sort menu from a track list. | The available sort fields are displayed and selecting one reorders the list. | `1400` returns at least one configured sort; unit tests verify exact ordering and query mapping. | Manuals: SORT on a track list. `[OBS]` |
| HW-BRW-06 | All | Select KEY in the category browser. | The 24 musical keys are shown; selecting a key leads to matching tracks. | RX3 `100b` or AZ/CDJ `1014` returns 24 keys. Unit tests cover RX3 exact-key and modern related-key requests. | Manuals: category browsing and Track Filter; firmware request families. `[OBS]` for RX3/CDJ, AZ `[ASSUME]`. |
| HW-INF-01 | All | Highlight a track and open its information/details view. | Title, artist, BPM, key, duration, and other available metadata are shown. | `2002` renders a non-empty metadata menu whose values come from the fixture track. | Manuals: browse information and loaded-track information. `[OBS]` |
| HW-INF-02 | All | Display a browse row or loaded-track view for a track without artwork. | The UI remains responsive and shows no artwork or a placeholder. | The model's artwork request receives `4002` with the captured no-art status instead of timing out. | Manuals: artwork fields; reply envelope from capture. Display placeholder `[UNKNOWN]` until hardware check. |
| HW-LOD-01 | All | Press LOAD for the highlighted track. | The selected deck loads the track and displays its title. | `2102` supplies the absolute path and size, then the emulator mounts the export and reads exact audio bytes over NFS. | Manuals: Loading a track; Link Export. `[OBS]` |
| HW-LOD-02 | All | View the loaded track. | BPM/beat positions and the beat grid are available to deck features. | `2204` returns the fixture beat-grid blob with success status. | Manuals: waveform/beat-grid displays; capture. `[OBS]` |
| HW-LOD-03 | All | Browse/highlight the analyzed track. | A preview waveform is shown in the track list where the model supports it. | `2004` returns the captured preview waveform with success status. | Manuals: Browse screen preview waveform. `[OBS]` |
| HW-LOD-04 | All | Load the analyzed track and open the waveform screen. | The enlarged/detail waveform is drawn. | `2904` returns the captured detail waveform with success status. | Manuals: waveform screen. `[OBS]` |
| HW-LOD-05 | All | Load a track containing memory/hot cues. | Cue and loop markers appear and can be selected by the deck. | `2504` returns the ordinary cue list. For the integration fixture, `2b04` returns its request-specific no-data envelope; a mocked unit case verifies successful extended-cue entries and count. | Manuals: cues, loops, and Hot Cues via Link Export; captures. `[OBS]` |
| HW-LOD-06 | All | Let the deck read the loaded track. | Playback can start and proceeds from the actual audio file. | NFS mount, path traversal, chunked reads, and EOF reconstruct bytes identical to the fixture file. | Link Export server capture and manuals' Link Export playback result. `[OBS]` transport; audible playback needs hardware. |
| HW-POST-01 | All | After loading, return to browse and continue using the RBX source. | The source remains present and the browser does not remain on “Waiting…”. | `3006` receives `4d02` with a 160-byte blob and `2602` renders 13 delivery-info rows. | CDJ-3000 firmware 3.20/capture `[OBS]`; accepted as compatibility behavior for AZ/RX3 `[ASSUME]`. |
| HW-RX3-01 | XDJ-RX3 | Load or deepen a track list on the RX3. | The same all-track content remains available during the transition. | RX3 profile uses `1200` and gets the same three fixture tracks as `1004`. | RX3 firmware path `dbcl_GetTrack_Content`. `[OBS]` |
| HW-RX3-02 | XDJ-RX3 | Request artwork during an RX3 load. | Artwork is displayed when present; missing artwork does not block loading. | RX3 profile uses `2103` and receives the required `4002` envelope. | RX3 firmware `dbcl_GetImage2`. `[OBS]` |

## Current AtEmu automation

The CDJ-3000 suite currently maps to the catalog as follows:

| Catalog IDs | Firmware test | Result established |
| --- | --- | --- |
| HW-NET-01, HW-SRC-01 | `test_the_app_joins_the_network`, `test_the_library_is_a_source_on_the_deck` | Stock firmware hears rekordbox player 17 and renders RBX on SOURCE. |
| HW-BRW-01, HW-LOD-01 through HW-LOD-06, HW-POST-01 | `test_the_first_track_loads_and_plays`, `test_stop_back_and_the_second_track_plays`, and the two app-strip tests | Firmware browses, loads over NFS, renders analysis, plays two distinct audio files, returns to browse, and reports both loads to RBX. |
| HW-BRW-02 | `test_playlist_opens_and_shows_its_tracks_in_fixture_order` | The firmware renders `Playlist 0` and all three fixture tracks in stored order. |
| HW-BRW-04 | `test_live_keyboard_search_returns_the_matching_track` | Live keyboard search renders matching tracks and clears them for a no-match suffix. |
| HW-BRW-05 | `test_key_header_sorts_by_the_wheel_on_the_player` | Selecting the KEY header reorders the visible firmware list as expected. |
| HW-INF-01 | `test_tag_track_button_and_loaded_track_rating_editor` | The firmware renders loaded-track INFO and persists a rating edit. |
| Additional edit/navigation behavior | the remaining `test_interactions.py` cases | Selection restoration, filters, tag list, rating propagation, grid correction, and nested BACK behavior operate through the live panel. |

HW-BRW-03 (HISTORY), HW-BRW-06 (KEY category navigation), and exact cue/artwork display still need dedicated panel assertions. Their RemoteDB requests remain covered by unit and protocol integration tests, which is weaker evidence. XDJ-RX3 firmware cases remain unavailable because that AtEmu model cannot boot.

The XDJ-AZ firmware suite exports a generated tone to a disposable FAT32
image, browses its playlist, loads deck 1, and checks playback and cue audio.
This establishes emulator/deck-1 coverage; it does not establish physical
XDJ-AZ or multideck behavior.

## Manual hardware run

Run this after the emulator suite passes and record model, firmware, connection type, RBX commit, date, packet capture path, and result for every catalog ID.

1. Start with a fixture library containing three distinct tracks, one two-track playlist, one history, artwork on one track, one track without artwork, beat grids, preview/detail waveforms, memory cues, and hot cues.
2. Quit rekordbox and other Pro DJ Link applications. Connect one target device and start RBX on the interface that reaches it.
3. Execute catalog IDs in order. Capture UDP 50000/50002, TCP 12523 and the returned database port, UDP 50111/2049, and mountd traffic.
4. Record the visible result separately from the packet result. A correct packet exchange with a wrong or stalled UI is a failure.
5. Repeat on XDJ-AZ, XDJ-RX3, and CDJ-3000. For all-in-one units, check each exposed logical deck without assuming that shared IP means shared player state.

Use this result format:

| ID | Device / firmware | Visible result | Protocol result | Pass | Evidence |
| --- | --- | --- | --- | --- | --- |
| HW-BRW-03 | CDJ-3000 / 3.20 | `LINK HISTORY …` shown; selecting it shows expected tracks | `1012`, render, `1112`, render | Yes/No | capture path + photo/video reference |

## Running the automated checks

Focused unit and protocol checks:

```sh
RB_LITE_TEST=1 cargo test -p rbl-dbserver --test session
RB_LITE_TEST=1 cargo test -p rbl-link --test link
```

For firmware integration results, confirm the intended app build,
device/firmware, test results, and a connected, ticking firmware panel.
A log message alone is not proof that the required behavior passed.

Before handing off a substantial Link Export change:

```sh
cargo clippy --workspace --all-targets -- -D warnings
RB_LITE_TEST=1 cargo test --workspace
```

XDJ-AZ's generated one-track USB fixture passes the emulated deck-1 audio gate;
XDJ-RX3 integration remains blocked until AtEmu has a bootable firmware model.
Their protocol dialects stay covered by unit and socket integration tests in
the meantime, with results labelled as protocol coverage.

## Coverage gate

A new Link Export behavior is ready for hardware testing when it has:

- a catalog ID with action and observable expected result;
- an evidence marker and source;
- a mocked unit test for query/state/encoding logic;
- an AtEmu assertion made by booted vendor firmware over the real transport path;
- an explicit empty/error result where data is absent;
- a named physical check for anything the emulator cannot prove.

It is complete only for the models and firmware actually exercised. A CDJ-3000 AtEmu pass does not establish XDJ-AZ or XDJ-RX3 compatibility, and no AtEmu pass replaces a named physical-device check where the model omits hardware behavior.
