# Library colour comments

Track colours retain their numeric IDs 1–8 and fixed palette dots. Their labels come from the owning database: `djmdColor.Commnt` for the master library, the `colors` table in the selected `export.pdb`, or `color.name` in the selected `exportLibrary.db`. Missing or empty entries use Pink, Red, Orange, Yellow, Green, Aqua, Blue and Purple respectively. Stored comments, including whitespace and text that matches a translated UI label, display literally.

Master names are read with the index and refreshed from their eight database rows when startup uses a cached snapshot. They do not change the snapshot format. Device names are read under an observational lease from the selected format, with no export recovery or settings publication. An unavailable device uses the stock fallback rather than another library's names.

The frontend shares reads between mounted views of one owner and retires that cache when the last view leaves. Newer refreshes and source changes supersede older replies. The table, colour filter, Info panel, deck INFO and collection smart-playlist editor share the same ID-based lookup; a filter remains attached to its numeric colour even if labels match or change. A device row retains its own ColorID when the same audio also appears in the master library.

Renaming applies to the master library only: Preferences › Advanced › Browse › Color, or a Color cell's right-click in a master-library table, writes `djmdColor.Commnt` (an empty name writes the stock one) and announces library-changed, which refreshes the master owner's views. Like a My Tag rename it is not recorded and ends the redo branch (`library-undo.md`). A device's names are edited in the device panel's Color tab; a device table offers no rename menu.

A USB with no `exportLibrary.db` is given the master library's names when it is first written (export or "create music database folders"), as rekordbox seeds a new device; a device that has its own keeps them (`usb-export-db.md`).
