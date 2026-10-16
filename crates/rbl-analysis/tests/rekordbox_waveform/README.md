# Waveform reference fixtures

The `.EXT` and `.2EX` files are rekordbox 7.2.19 output for the six
20-second stereo signals in `signals.rs`, as described in the algorithm
reference. `rekordbox_waveform_fixtures` writes those exact WAV inputs.

The `.blue-preview` files each contain **400 PWAV bytes followed by 100
PWV2 bytes**. These were captured on 2026-10-16 by executing the isolated
`BeatAnalyzer_2_0::LoudViewMaker` routines from rekordbox **7.2.11.0342**
(macOS arm64). The application and its library were not opened. They are
native-routine outputs, not a capture of a complete rekordbox analysis job.

Reference arm64 Mach-O SHA-256:
`4677adf5a11e154b0c9dad3d6e2bee1f8205836351d5b7f2985fce3a84a36967`.

The native constructor and `create(rate, frames)` initialize the pass;
`analyzePeak` receives the integer stereo mean divided by 32768, in blocks
of `floor(rate/75)` frames, with null optional band inputs. The final block
ends on the actual last frame. `analyzeLoudColor(0,400)` supplies colors;
`V_Devide` and `notifyDots` supply the height thresholds; `MstStoreLoudWave`
establishes the packed color/height encoding. No native executable code is
included in these fixtures or in the implementation.

`kick-hats-long.blue-preview` uses the exact `kick-hats` frames repeated ten
times (200 seconds). `kick-hats-tail.blue-preview` appends the first 137
frames to exercise a fractional final block. The other files use the six original signals. All PCM
is generated here; no recorded music is included.

[OBS] Both sections match every fixture byte. This establishes the stated
native-routine boundary; it does not establish every rekordbox version,
analyzer input path, player rendering, or physical-device compatibility.
