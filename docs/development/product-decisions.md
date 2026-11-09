# Product decisions

## Project vision

RBXport aims to be a fast Rekordbox clone with a deliberately limited feature
set. It trades feature breadth for speed and responsiveness, including on older
machines. The core focuses on library management, USB exporting, and PRO DJ
LINK, with Rekordbox behavior and compatibility within that scope.

The core does not aim to become a full DJ performance application or an
advanced library manager beyond what Rekordbox offers. A Rekordbox counterpart
is necessary for a new feature, but does not guarantee inclusion.

RBXport also aims to provide a useful foundation for others to fork and create
their own DJ software. Forks may pursue broader features and different goals
while the core stays focused and fast.

## Auto Relocate search-folder preferences

On 2026-11-08, Chris accepted completing the Rekordbox-style Auto Relocate
search-folder preferences for the version after the upcoming 1.11 release. Keep
this feature out of 1.11 and its integration batch. Track
remaining behavior and evidence gaps under #415 and the preferences UI under
#487; this scheduling decision does not certify parity or current UI visibility.
