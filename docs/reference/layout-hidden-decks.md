# Layout switches and the hidden deck

Issue #548 asked for deck 2 to stop when the layout changes to 1 PLAYER,
SIMPLE PLAYER or FULL BROWSER. This records what rekordbox does to the decks a
layout hides, the evidence for it, and what RBXport does with that.

## Reference behavior

Read-only static inspection of the installed rekordbox 7.2.11.0342 macOS
arm64 executable (SHA-256 of the arm64 slice
`4677adf5a11e154b0c9dad3d6e2bee1f8205836351d5b7f2985fce3a84a36967`) traces an
EXPORT-mode layout command as follows. Addresses are from that build.

- `MainComponent::changeLayout(CommandIDs)` at `0x100806438` maps the menu
  command to a skin and a layout mode: command `0xb040` is
  `singleplayer.xml`, mode `0x11` (1 PLAYER); `0xb041` is
  `simpleplayer.xml`, mode `0x1001` (SIMPLE PLAYER); `0xb042` is
  `hideplayer.xml`, mode `0` (FULL BROWSER); `0xb043`/`0xb058` are
  `simplemixer.xml`/`newsimplemixer.xml`, mode `0x12` (2 PLAYER). It stores
  the mode in `PlayerLayout` first, then applies the skin through
  `MainComponent::changeLayout(String, String, int)` at `0x100805ba4`, directly
  or deferred through `LayoutChanger::internalCall`.
- That overload saves the `WindowLayout` setting, calls
  `UiManager::notifyChangeLayout` (both mixers, both players, the Export
  player panel) and `UiManager::initSkin("skins", false)` at `0x1015363ac`.
  With its flag false, `initSkin` repaints the four players
  (`UiPlayer::repaintWidgetPlayer`) and ends in
  `UiManager::resizePlayerComponent` at `0x101536848`. The flag-true branch,
  `UiPlayer::initWidgetPlayer` → `UiPlayer::stopSongFile` →
  `DjEngineIF::pause`, is the only path from `initSkin` to the engine's
  transport, and only `UiManager::setupComponents` (startup) passes true.
- `resizePlayerComponent` sizes the player component by `WindowLayout`
  (`singleplayer.xml` one deck high, the two-player skins two decks high,
  `simpleplayer.xml` a strip, `hideplayer.xml` nothing or the REC panel),
  calls `setBounds` and then the component's `resized()` explicitly (vtable
  slot `0x120` of `PlayerComponent` resolves to `PlayerComponent::resized`
  at `0x101da6dc0`), which lays out the player panel.
- `PlayerComponent::PlayerPanelL::resized()` at `0x101d9b6fc` ends, on every
  path, in the block at `0x101da0748`: when `PlayerLayout::getLayout()` is
  `0`, `0x11` or `0x1001`, it walks engine channels 1 to the player count
  (`PlayerComponent` field `0x114`) and calls `DjEngineIF::pause(channel)` on
  each one that `isPlaying`; channel 0, deck 1, is skipped. It then turns
  master tempo off for every channel in EXPORT mode. In the other layouts the
  same block restores each panel's sync toggle and master tempo from settings
  instead.
- Nothing in the traced path calls `DjEngineIF::play`. The direct callers of
  `DjEngineIF::play` in this build are the play/pause switch, the waveform's
  play button, Automix and the lighting macro editor, so a deck paused by the
  switch stays paused when 2 PLAYER returns. Nothing unloads it either:
  `UiPlayer::unloadSongFile` is unreachable from the layout entry points by
  direct call within nine levels.
- `MainComponent::changeLayout(CommandIDs)` also ends in
  `PlayerComponent::requestTempoReset`, which in these three layouts resets
  every player control panel's tempo (`PlayerControllPanel::resetTempo`).
  That is a tempo effect, not a transport one, and RBXport does not copy it
  here.

So, `[OBS]` for 7.2.11 static: switching to 1 PLAYER, SIMPLE PLAYER or FULL
BROWSER pauses every playing deck but deck 1 at its current position, deck 1
keeps playing (including in FULL BROWSER, which `UiPlayer::timerSongTime` at
`0x101705684` also relies on when it goes on to the next track there), and
switching back to 2 PLAYER redisplays the hidden deck paused and still loaded.

The step from the player component's `resized()` to the panel's is JUCE's
`setBounds`, which calls `resized()` when the size changes; the three
layouts give the panel a different height from 2 PLAYER's, so this is
`[ASSUME]` on JUCE semantics rather than a traced direct call.

`[UNKNOWN]`: the same trace on 7.2.19, the build other parity notes cite.
No 7.2.19 executable is on this machine, and the installed build is 7.2.11.
The maintainer's 7.2.19 note on #548 traced the layout notification and the
Export panel but not `PlayerPanelL::resized`, so it neither confirms nor
contradicts this. No running-app observation was made: the reference check
that would close both gaps is, in rekordbox EXPORT mode with two generated
tracks, deck 2 playing, switch to each of the three layouts, switch back, and
compare deck 2's playhead with the elapsed time.

## RBXport behavior

The layout menu and native layout commands use the same application handler.
When a switch from 2 PLAYER hides deck B, the handler pauses it in the engine
and leaves deck A playing. Deck B keeps its track and position. Returning to
2 PLAYER reads the paused transport state from the engine.

`e2e/hidden-deck-transport.spec.ts` covers each switch and then returns to
2 PLAYER to verify both decks. It also covers selecting 2 PLAYER again.
