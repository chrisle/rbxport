# Product decisions

## Rekordbox-only scope

RBXport aims to be a fast Rekordbox clone with a deliberately limited set of
features. It trades feature breadth for speed and responsiveness, including on
older machines. The core focuses on library management, USB exporting, and
PRO DJ LINK; it does not aim to become a full DJ performance application or
an advanced library manager beyond what Rekordbox offers. A Rekordbox
counterpart is necessary for a new feature, but does not guarantee inclusion.

The project also aims to provide a useful foundation for others to fork and
create their own DJ software. Forks can expand the product in other directions
while the core stays focused and fast.

RBXport is solely for Rekordbox. Integrations, library import/export, and export
databases for other DJ software, including Mixxx and Engine DJ, are outside
scope. An opt-in setting or shared audio files does not change that decision.
Existing safety, correctness, and compatibility work remains necessary.

## Vocal and phrase detection

RBXport will not add vocal or phrase detection, including custom vocal/phrase-marked
export waveforms. It should remain useful on older machines.

Bundling or downloading AI models adds size and resource costs. Commercial
licensing requirements for suitable separation models would also complicate
keeping RBXport open source. These concerns govern the decision regardless of
how AlphaTheta packages or prices the features.

Existing analysis support and phrase data already present in a library remain
within the existing compatibility scope.

## Advanced metadata management

RBXport will not add an advanced metadata cleanup suite: batch find/replace,
automatic capitalization, conditional transformations, whitespace cleanup,
or automatic artist/title splitting. This functionality is outside the
Rekordbox feature scope RBXport aims to reproduce.

For advanced library and database management, we recommend the excellent
[Lexicon DJ](https://www.lexicondj.com/) app. Ordinary metadata editing remains
within RBXport's existing scope; a Lexicon integration does not.
