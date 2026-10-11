# MCP server

[Documentation](../README.md)

rbxport can run a [Model Context Protocol](https://modelcontextprotocol.io)
server. An AI assistant on this computer can then search the library, read
the play history, and build playlists, for example Claude Code, Claude
Desktop, Codex, or another MCP client.

The server is off by default. To turn it on, open Preferences → Advanced →
Browse → MCP server and turn on **Let AI assistants use the library**.
Preferences then shows the address the server listens on:

```text
http://127.0.0.1:47200/mcp
```

The server runs only while rbxport is open. To use a different port, set
`RBXPORT_MCP_PORT` before you start rbxport.

## Safety

- The server listens on `127.0.0.1` only. Other computers cannot connect to it.
- It refuses requests from web pages: a request that has a browser `Origin`
  header, or a `Host` other than `localhost`, `127.0.0.1` or `::1`, gets a
  403 error.
- An assistant edits your real library. It uses the same edit path as the
  window, with the same rules:
  - **Library Protection** blocks every edit. With protection on, an
    assistant can search and read, but it cannot change anything.
  - Edits are refused while rekordbox is running.
  - Undoable edits go into the window's Undo history. These include
    renaming, moving, or deleting a playlist, removing tracks from a playlist,
    and changing tags, rating, colour, or comment. An assistant can call
    `undo_last_edit`.
  - Two tools cannot be undone: `remove_tracks_from_collection` and
    `remove_missing_tracks`. Make a backup before you let an assistant clean
    up the library.
- Any program on this computer can connect while the server is on. Turn the
  server off when you do not use it.

## Connecting an assistant

Use the address from Preferences. The transport is Streamable HTTP.

Claude Code:

```sh
claude mcp add --transport http rbxport http://127.0.0.1:47200/mcp
```

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.rbxport]
url = "http://127.0.0.1:47200/mcp"
```

A client that uses only stdio, such as Claude Desktop, needs a bridge, for
example [`mcp-remote`](https://www.npmjs.com/package/mcp-remote):

```json
{
  "mcpServers": {
    "rbxport": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "http://127.0.0.1:47200/mcp"]
    }
  }
}
```

## Tools

Track, playlist, and history ids are decimal strings. A tool that returns
tracks returns at most 200 tracks per call. Use `offset` to get the next page.

### Read

| Tool | What it does |
| --- | --- |
| `library_status` | Track and playlist counts. Tells whether edits are possible now, and if not, why. |
| `search_tracks` | Search the collection, a playlist, or a history session. Filters: search words, artist, genre, album, label, file name, keys, BPM range, minimum rating, colours, play count, year, date added, and analysis state. Sort by any column. |
| `get_track_details` | All fields of up to 50 tracks: file path and format, cue points, My Tags, composer, remixer, and more. |
| `validate_track_ids` | Tells which ids are tracks in the library. |
| `list_playlists` | The playlist tree in order, with ids, paths, and counts. |
| `get_playlist_tracks` | The tracks of a playlist in playlist order. For a folder, the tracks of all playlists under it. For an intelligent playlist, the tracks its rule matches now. |
| `list_history_sessions` | The DJ history sessions, newest first. |
| `get_session_tracks` | The tracks of a session, in the order they were played. |
| `get_history_stats` | Session and play counts, the most-played tracks, and the top genres and artists. |
| `get_library_stats` | Totals, average BPM, and the top genres, keys, and artists. |
| `analyze_library` | Group the library, a playlist, or a session by genre, artist, album, label, key, year, rating, colour, BPM band, or month added. |
| `find_related_tracks` | Related Tracks: tracks that match by BPM and key, by artist, or by genre among tracks added recently, or tracks that came next in your histories. |
| `find_duplicates` | Tracks with the same title and artist. |
| `find_missing_tracks` | Tracks whose audio file is not found. |

Keys can be musical names (`Am`, `F#`) or Camelot codes (`8A`). Set
`compatible_keys` in `search_tracks` to also get the keys that mix
harmonically with them.

### Edit

| Tool | What it does |
| --- | --- |
| `create_playlist` | Make a playlist or folder. Returns its id. |
| `rename_playlist`, `move_playlist`, `delete_playlist` | Change the playlist tree. |
| `add_tracks_to_playlist`, `remove_tracks_from_playlist`, `reorder_playlist` | Change what a playlist contains and its order. |
| `update_tracks` | Set the rating, colour, comment, or tags. Title and BPM can be set on one track at a time only. |
| `import_tracks` | Add audio files or folders to the collection. Open rbxport to analyse them. |
| `remove_tracks_from_collection` | Remove tracks from the library and from every playlist. The files stay on disk. Cannot be undone. |
| `remove_missing_tracks` | Remove the tracks whose files are missing. Cannot be undone. |
| `undo_last_edit` | Undo the last undoable edit. |

Example prompts:

- "Make a playlist of my 5-star techno between 128 and 132 BPM that I have
  not played in a set yet."
- "What did I play most this year, and which genres?"
- "Order the tracks in Friday so that each key is compatible with the next."
