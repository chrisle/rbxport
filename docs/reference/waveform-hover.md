# Waveform hover destinations

A translucent white line previews the existing click destination in browser row
waveforms and deck overviews, including the Simple Player and both decks. It is
separate from the current playback head. Browser hot-cue badges use their actual
cue-click destination. Moving the pointer alone never starts or seeks audio.

The line clears on leave, press/drag/cancel, keyboard input, scrolling, focus loss
or source replacement. Touch input does not leave a hover indicator. Cue display
preferences, cached waveform rendering and existing press/capture handling remain
independent; the overlay updates directly without per-move React state or IPC.

## Enlarged waveform hover behavior

The enlarged scrolling waveform keeps its record-style relative drag and the
optional click for PLAY and CUE. The pointer does not mark a seek destination.
Static analysis of the rekordbox 7.2.11 macOS arm64 build found the same distinction
between the overview and the enlarged waveform `[OBS static]`. Symbols were read
with `nm` and function bodies with `objdump`.

- The deck overview, `djplay::WidgetLoudView`, stores the clamped pointer x in
  `mouseMove` (@0x100daa6f0) and resets it to -1 in `mouseExit`, `mouseDown` and
  `mouseDrag`. `paintWave` (@0x100da8864) draws a one-pixel
  `juce::Graphics::drawVerticalLine` at that x with alpha 127 while it is set.
  Its press and drag report an absolute 0..400 position to the widget operator.
  This is the behavior the browser previews and deck overviews mirror.
- The enlarged waveform, `djplay::WidgetWaveView`, uses `mouseMove`
  (@0x101c712b0) for song-structure border handling and, only while MIX POINT
  LINK mode is on (`PlayerWaveView::setMixPointLinkMode` @0x100aebd1c), records
  the pointer position for that mode. No function in `WidgetWaveView`,
  `PlayerWaveView`, `WaveViewArea` or `PlayerComponent::WidgetPlayer` reads that
  position back. `mouseExit` (@0x101c713fc) restores the normal cursor.
- `WidgetWaveView::mouseDrag` (@0x101c7197c) turns pointer movement into a jog
  speed that `UiPlayer::eventWaveView` (@0x101d3dbf4, event 10) passes to
  `DjEngineIF::setJogSpeed`; `mouseUp` sends event 11, `DjEngineIF::stopJogSpeed`.
  The left button's press and release (event 6) reach `DjEngineIF::touchJog`.
  The only position write in that event switch is the relative
  `setPlayingTime` of event 9.
- The rekordbox 7.2.18 manual describes the enlarged waveform as drag to move
  the playback point and click for PLAY/CUE under the same preference, and
  documents no hover indicator for it.

MIX POINT LINK is outside the RBXport comparison. The exact rekordbox PLAY/CUE
click dispatcher and runtime hover behavior remain `[UNKNOWN]`. The RBXport
Playwright hover spec asserts that the detail surface has no hover head.
