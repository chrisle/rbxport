# Waveform hover destinations

A translucent white line previews the existing click destination in browser row
waveforms and deck overviews, including the Simple Player and both decks. It is
separate from the current playback head. Browser hot-cue badges use their actual
cue-click destination. Moving the pointer alone never starts or seeks audio.

The line clears on leave, press/drag/cancel, keyboard input, scrolling, focus loss
or source replacement. Touch input does not leave a hover indicator. Cue display
preferences, cached waveform rendering and existing press/capture handling remain
independent; the overlay updates directly without per-move React state or IPC.

The enlarged scrolling waveform keeps its existing record-style relative drag
and optional click Play/CUE behavior. This change does not add an absolute seek
indicator there; that part of #859 remains outside the verified scope. Exact
vendor hover behavior across all waveform surfaces has not been runtime-tested.
