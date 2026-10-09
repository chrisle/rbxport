/**
 * The mock's USB libraries: what the TEST stick rekordbox wrote holds, in
 * both of its libraries, so the Devices tree can be browsed and edited in a
 * browser. Edits change one library and leave the other, as the real
 * backend's do.
 */
import type {
  DeviceFormat, DeviceLibrary, DevicePlaylistEdit, DevicePlaylistEditResult, RowDto,
} from "./types";

interface MockNode {
  id: number;
  parent: number;
  name: string;
  folder: boolean;
  sequence: number;
  tracks: number[];
}

interface MockLibrary {
  format: DeviceFormat;
  nodes: MockNode[];
}

interface MockTrack {
  id: number;
  title: string;
  artist: string;
  bpmX100: number;
  key: string;
  durationSec: number;
}

/**
 * Stand-in tracks. The playlists' names, ids and order are the real TEST
 * stick's [OBS: read-only copy, 2026-10-08]; the tracks are made up.
 */
const TRACKS: readonly MockTrack[] = [
  { id: 1, title: "B2ME (feat. JordinLaine) (Original Mix)", artist: "atDusk", bpmX100: 12_800, key: "Bbm", durationSec: 390 },
  { id: 2, title: "Spectrum (Extended Mix)", artist: "Arty", bpmX100: 13_200, key: "Fm", durationSec: 402 },
  { id: 3, title: "Lost Without You", artist: "Ferry Corsten", bpmX100: 13_800, key: "Am", durationSec: 365 },
  { id: 4, title: "Gravity", artist: "Gareth Emery", bpmX100: 13_000, key: "Gm", durationSec: 344 },
  { id: 5, title: "Mirage", artist: "Tinlicker", bpmX100: 12_300, key: "Dm", durationSec: 421 },
  { id: 6, title: "Cold Blooded", artist: "Ilan Bluestone", bpmX100: 12_600, key: "Ebm", durationSec: 377 },
];

function testLibrary(format: DeviceFormat): MockLibrary {
  return {
    format,
    nodes: [
      { id: 3, parent: 0, name: "NP3-TEST-MP3", folder: false, sequence: 0, tracks: [5, 6] },
      { id: 2, parent: 0, name: "Melodic Vox", folder: false, sequence: 1, tracks: [2, 3, 4] },
      { id: 1, parent: 0, name: "Now Playing Test", folder: false, sequence: 2, tracks: [1, 2, 3, 4] },
    ],
  };
}

export interface MockDeviceLibraries {
  libraries(path: string): DeviceLibrary[];
  edit(path: string, format: DeviceFormat, edit: DevicePlaylistEdit): DevicePlaylistEditResult;
  /** A playlist's rows in its order, or every track for `"0"`. */
  rows(path: string, format: DeviceFormat, playlist: string): RowDto[];
}

/** Depth first from the top level, siblings by sequence then id. */
function inTreeOrder(nodes: readonly MockNode[]): { node: MockNode; depth: number }[] {
  const out: { node: MockNode; depth: number }[] = [];
  const visit = (parent: number, depth: number) => {
    const children = nodes
      .filter((n) => n.parent === parent)
      .sort((a, b) => a.sequence - b.sequence || a.id - b.id);
    for (const node of children) {
      out.push({ node, depth });
      if (node.folder) visit(node.id, depth + 1);
    }
  };
  visit(0, 0);
  return out;
}

function trackRow(path: string, track: MockTrack, position: number): RowDto {
  const fileName = `${track.title}.mp3`;
  return {
    id: `file:${path}/Contents/${track.artist}/${fileName}`,
    trackNo: position,
    title: track.title,
    artist: track.artist,
    album: "",
    genre: "Trance",
    label: "",
    comment: "",
    bpmX100: track.bpmX100,
    key: track.key,
    durationSec: track.durationSec,
    rating: 0,
    analysed: 0,
    dateAdded: "2026-10-08",
    releaseDate: "",
    hotCues: [],
    artworkHue: 0,
    hasArtwork: false,
    fileName,
  };
}

export function createMockDeviceLibraries(): MockDeviceLibraries {
  const sticks = new Map<string, MockLibrary[]>([
    ["/Volumes/TEST", [testLibrary("deviceLibrary"), testLibrary("oneLibrary")]],
  ]);
  const libraryOf = (path: string, format: DeviceFormat): MockLibrary => {
    const library = sticks.get(path)?.find((l) => l.format === format);
    if (!library) throw new Error("That device is no longer connected.");
    return library;
  };
  const trackByRow = (path: string, row: string): number => {
    const track = TRACKS.find((t) => trackRow(path, t, 0).id === row);
    if (!track) throw new Error("Only tracks of this library on the device can go into its playlists.");
    return track.id;
  };

  return {
    libraries: (path) =>
      (sticks.get(path) ?? []).map((library) => ({
        format: library.format,
        tracks: TRACKS.length,
        nodes: inTreeOrder(library.nodes).map(({ node, depth }) => ({
          id: String(node.id),
          parentId: String(node.parent),
          name: node.name,
          folder: node.folder,
          depth,
          count: node.folder ? library.nodes.filter((n) => n.parent === node.id).length : node.tracks.length,
        })),
      })),
    rows: (path, format, playlist) => {
      const library = libraryOf(path, format);
      const ids = playlist === "0"
        ? TRACKS.map((t) => t.id)
        : library.nodes.find((n) => String(n.id) === playlist)?.tracks ?? [];
      return ids.flatMap((id, at) => {
        const track = TRACKS.find((t) => t.id === id);
        return track ? [trackRow(path, track, at + 1)] : [];
      });
    },
    edit: (path, format, edit) => {
      const library = libraryOf(path, format);
      const nodes = library.nodes;
      const find = (id: string) => nodes.find((n) => String(n.id) === id);
      const gone = () => new Error("That playlist is no longer on the device.");
      switch (edit.kind) {
        case "create": {
          const parent = Number(edit.parent);
          if (parent !== 0 && find(edit.parent)?.folder !== true) throw new Error("That folder is no longer on the device.");
          const name = edit.name.trim();
          if (name === "") throw new Error("A playlist needs a name.");
          // At the top of its parent, the others moved down one, as
          // rekordbox does [OBS 7.2.14, Winrig 2026-10-08].
          const id = Math.max(0, ...nodes.map((n) => n.id)) + 1;
          for (const sibling of nodes) if (sibling.parent === parent) sibling.sequence += 1;
          nodes.push({ id, parent, name, folder: edit.folder, sequence: 0, tracks: [] });
          return { id: String(id), changed: 1 };
        }
        case "rename": {
          const node = find(edit.id);
          if (!node) throw gone();
          const name = edit.name.trim();
          if (name === "") throw new Error("A playlist needs a name.");
          const changed = node.name === name ? 0 : 1;
          node.name = name;
          return { id: edit.id, changed };
        }
        case "delete": {
          if (!find(edit.id)) throw gone();
          const doomed = new Set([Number(edit.id)]);
          for (let grew = true; grew;) {
            grew = false;
            for (const n of nodes) {
              if (doomed.has(n.parent) && !doomed.has(n.id)) {
                doomed.add(n.id);
                grew = true;
              }
            }
          }
          const parent = find(edit.id)?.parent ?? 0;
          library.nodes = nodes.filter((n) => !doomed.has(n.id));
          library.nodes
            .filter((n) => n.parent === parent)
            .sort((a, b) => a.sequence - b.sequence || a.id - b.id)
            .forEach((n, at) => { n.sequence = at; });
          return { id: edit.id, changed: doomed.size };
        }
        case "add":
        case "remove": {
          const node = find(edit.playlist);
          if (!node) throw gone();
          if (node.folder) throw new Error("A folder holds playlists, not tracks.");
          const tracks = edit.tracks.map((row) => trackByRow(path, row));
          const before = node.tracks.length;
          if (edit.kind === "add") {
            for (const track of tracks) if (!node.tracks.includes(track)) node.tracks.push(track);
            return { id: edit.playlist, changed: node.tracks.length - before };
          }
          node.tracks = node.tracks.filter((t) => !tracks.includes(t));
          return { id: edit.playlist, changed: before - node.tracks.length };
        }
      }
    },
  };
}
