# AppleScript automation

[Documentation](../README.md)

On macOS, rbxport can be scripted: from Script Editor, `osascript`,
Shortcuts' "Run AppleScript", or any app that sends Apple Events. Open the
dictionary with Script Editor → File → Open Dictionary… → rbxport to see
every class, property and command.

The first time a program scripts rbxport, macOS asks whether to allow it
(System Settings → Privacy & Security → Automation).

## Library write protection

Every change to the library is refused while rekordbox is running, and
while Library Protection is on (it is by default). Turn it off from a
script with `set value of setting "advanced.protectLibrary" to false`, or in
Preferences. A refused change raises an error with the same message the
window shows.

## Objects and properties

| Object | What it is |
| --- | --- |
| `track` | A track in the collection. `name` (the title), `artist`, `album`, `genre`, `label`, `key`, `bpm`, `year`, `rating` (0 to 5 stars), `color`, `comment` and `play count` can be set. `duration`, `date added`, `location`, `analysed`, `bit rate`, `sample rate` and `id` are read-only. |
| `playlist` | A playlist, folder or smart playlist, at any depth. `name` can be set; `kind`, `parent`, its `tracks` and (for a folder) its `playlists` are read. |
| `deck` | The window's players: `deck 1` is player A, `deck 2` player B. `current track`, `playing`, `player position` and `duration` in seconds, `tempo` in percent. |
| `device` | A volume a playlist can be exported to. |
| `link player` | A CDJ heard over PRO DJ LINK. Its `id` is its player number. |
| `setting` | A Preferences choice, named `pane.field`, such as `"advanced.protectLibrary"` or `"view.keyDisplay"`. `value` can be set. |

The application itself has `link export` (set it to turn LINK on or off) and
`rekordbox running`.

Track and playlist ids are text: playlist ids run to 2^32, past the largest
integer AppleScript holds, and track ids follow suit. `track id "12345"`.

## Commands

- `play`, `pause` — a deck, `deck 1` when none is given. They press the
  deck's PLAY, so BEAT SYNC and quantize behave as they do for a click.
- `load` *track* `into` *deck or link player* — `deck 1` when none is
  given. A deck load waits until the track is ready to play.
- `add` *tracks* `to` *playlist*, `remove` *tracks* `from` *playlist*.
- `export` *playlist* `to` *device* — writes the playlist to the stick
  with the DJ System and USB Export preferences, and returns the summary
  the window shows. Wrap a long export in `with timeout`.
- `make new playlist`, `delete`, `move` — for playlists and folders.
  Deleting a track of a playlist takes it off that playlist; it stays in
  the collection.

## Examples

```applescript
tell application "rbxport"
    -- The tracks of a playlist
    get name of every track of playlist "Friday"

    -- Rate everything by one artist
    set rating of (every track whose artist is "Octave One") to 5

    -- A folder with a playlist in it
    set sets to make new playlist with properties {name:"Sets", kind:playlist folder}
    set warmup to make new playlist at sets with properties {name:"Warm-up"}
    add (every track whose bpm < 122) to warmup

    -- Preview on the first deck
    load (first track whose name contains "Blackwater") into deck 1
    play deck 1

    -- To a stick, with time for a big playlist
    with timeout of 3600 seconds
        export playlist "Friday" to device "DJ STICK"
    end timeout
end tell
```

## Developer entry points

### My Tag automation (catalog schema 1)

Companions can read the existing taxonomy and add/remove assignments without
replacing a track's other tags. Tag IDs are text, scoped to a library. Names
are display labels, not identifiers; the same name can occur in two categories.

```applescript
tell application "rbxport"
    -- JSON text: schema_version, library_id, capabilities, categories, tags.
    set catalogJSON to my tag catalog
    -- Substitute IDs obtained from this library's catalog and collection.
    set currentIDs to read my tags track id "12345"
    set didChange to add my tags {"67", "89"} to track id "12345"
    set didChange to remove my tags {"67"} from track id "12345"
end tell
```

To change the order My Tags are listed in, name every category, or every tag
of one category, in the new order:

```applescript
tell application "rbxport"
    -- Every tag of category "10", first to last.
    set didChange to reorder my tags {"89", "67", "71"}
    -- Every category, first to last. Their tags keep their own order.
    set didChange to reorder my tags {"11", "10", "12", "13"}
end tell
```

`my tag catalog` returns this shape (example IDs and names):

```json
{
  "schema_version": 1,
  "library_id": "123456789",
  "capabilities": ["my_tags.read", "my_tags.add", "my_tags.remove", "my_tags.reorder"],
  "categories": [{"id": "10", "name": "Texture"}],
  "tags": [{"id": "67", "name": "grinding", "category_id": "10"}]
}
```

Categories are listed in their order. Tags are listed category by category, in
the categories' order, and in each category's own order.

An unavailable library identity is `null`, never a shared sentinel ID. A
companion should verify the identity before reusing saved mappings and require
explicit remapping if it is unavailable or changes. Capabilities describe only
this catalog's My Tag operations. Clients should check `schema_version` and
ignore unfamiliar extra fields. Existing playlist automation is unchanged.

`read my tags` returns a list of text IDs. Both mutation commands return `true`
when membership changes and `false` for a no-op. Duplicate inputs, repeated
additions, repeated removals, and empty lists are safe. Each command validates
all supplied IDs and updates **one track atomically** using the current database
membership, preserving other assignments. Multiple commands are not one
transaction. Unknown, deleted, or category IDs are refused, even on removal.

`reorder my tags` orders one list at a time, as rekordbox's My Tag panel
does: the categories, or the tags inside one category. The list must name
every live entry of that one list exactly once, and nothing else. Unknown,
deleted or repeated IDs, a missing ID, an ID from another category, or a mix of
categories and tags are refused with nothing written. Reordering never creates,
deletes, renames or moves a tag to another category, and never changes which
tracks carry a tag. It returns `true` when the order changed and `false` when
the library already had it. The stored order is renumbered from 1 in the order
given, as rekordbox renumbers it after a drag. Undo puts back the exact earlier
order and is refused if a tag has been added, removed or moved in that list
since.

Changes use the same library writer, undo history, index refresh, and events as
UI edits. Library Protection is checked again when the queued edit runs; the
writer retains its Rekordbox-running and recovery guards. Native My Tags are
database assignments: these commands do not embed tags into audio files or
modify beat grids, cues, or ordinary track fields. That includes the comment: with
Preferences > Advanced > Browse > Add "My Tag" to the "Comments" on, only an
edit in the My Tag panel or the Info tab rewrites it, as rekordbox rewrites it
only after an edit in its My Tag panel. The existing scripting error
conventions apply: malformed ID lists are `-1703`, unresolved track objects are
`-1728`, and writer refusals (including unknown tag IDs) are `-10003` with a
reason. Read/database failures are errors rather than empty successful results.

Creation, renaming, merging, deletion, and moving tags between categories are
not exposed.
Create the initial vocabulary in the library first, then map companion labels
to its IDs. Model results, evidence, drafts, and publication/ownership history
belong in the companion. These operations do not provide Lexicon synchronization.

### Implementation

The macOS integration is in `src-tauri/src/scripting/`. Scripted library edits
must preserve the same guards and refresh behavior as UI edits. Use disposable
libraries for writable automation tests; see [Conventions](../development/conventions.md)
and [Testing](../development/testing.md).

### Checked collection removal (companion extension)

Our branch also exposes `remove collection tracks` with a JSON text argument:
`{"library_id":"123","tracks":[{"id":"456","path":"/Users/dj/Music/track.wav"}]}`.
It returns true on success. Supply 1–200 entries. Library identity, stored file
paths and other live references to the same paths are checked inside the write
transaction. A changed identity/path or an unselected reference refuses the
whole batch. Retries may include already tombstoned tracks with the same path.

This uses Remove from Collection's existing writer and refresh path: collection
and playlist memberships are soft-deleted, audio is untouched, and undo history
is cleared. Library Protection and the running-Rekordbox guard still apply.
The catalog advertises `collection.remove_checked`. Companions must separately
coordinate and journal file removal and any other library, such as Lexicon.

### Read an existing waveform overview

Companions can request the stored three-band overview without starting analysis:

```applescript
tell application "rbxport"
    set overview to read waveform overview "{\"library_id\":\"42\",\"track_id\":\"10000\"}"
end tell
```

Use the current `library_id` from `my tag catalog` and a collection track ID;
the example IDs are placeholders. Detect `waveform.overview.read` in the catalog
before calling. The version 1 JSON response echoes both IDs, sets `kind` to
`"bands"`, and contains a `data` byte array of low/mid/high triples (at most
1,200 columns / 3,600 bytes). An empty array means no overview is available,
including a track no longer in the index. A mismatched library identity is an
error. No audio or analysis files are written. See the
[companion waveform reference](../development/companion-waveform.md).
