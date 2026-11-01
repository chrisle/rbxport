# FAQ

[Documentation](../README.md)

## What is RBXport for?

RBXport manages a rekordbox library and exports it to USB sticks and PRO DJ
LINK. It is not DJ performance software. See the [README](../../README.md) for
its scope.

## Can I register or map a DJ controller?

No. RBXport does not register DJ controllers, map their buttons, pads, or
knobs, and has no performance mode. This includes models such as the Pioneer
DDJ-FLX4, DDJ-FLX8, and DDJ-FLX10. A missing controller setup or performance
mode is not a bug in RBXport.

To set up a controller or use its performance mode, use rekordbox or the
controller's own software.

## Can I manage my library in RBXport and use it in rekordbox performance mode?

Yes, one after the other. RBXport has no library of its own: it opens the
library rekordbox is set to use (the `master.db` and `share` folder named by
`masterDbDirectory` in rekordbox's settings), so rekordbox opens the same
library without any conversion or setup. RBXport still has no performance mode.
Open rekordbox to use its performance mode and mixer.

Use them one at a time. RBXport refuses to change the library while rekordbox
or its agent is running, so quit rekordbox before editing in RBXport, then
quit RBXport before reopening rekordbox. Restoring a backup and syncing a USB
stick also need rekordbox closed; creating a backup does not.

To use a different library in both apps, switch it in RBXport under
Preferences → Advanced → Database → Database management, or in rekordbox under
Preferences → Advanced. Either one sets the location the other reads. See
[Library location](../reference/library-location.md).
