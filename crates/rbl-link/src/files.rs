//! The file tree a player reads audio from.
//!
//! rekordbox exports the host's whole filesystem root as `/` and resolves
//! every name a player asks for against the host. The export here is the
//! same root, but a player can only reach the library's folders: each
//! track's path is cut at its first three components (`/Volumes/SD/RB`,
//! `/Users/me/Music`) and that folder is allowed as a whole, so a name
//! under it is answered by the host when a player asks and nothing is
//! built up front. A 38,000-track library in tens of thousands of folders
//! comes to a handful of roots, found in one pass. A track that sits
//! shallower than that is allowed by itself.
//!
//! The path a player asks for is the absolute host path the track-info reply
//! gave it, so the tree must be rooted where those paths are. On macOS that
//! is one export, `/`. On Windows rekordbox's paths are `C:/Users/…` and the
//! player mounts `/C/` — `[ASSUME]` from alphatheta-connect's client, which
//! resolves a Windows path that way; not yet measured against a capture on
//! Windows.

use rbl_index::Library;
use rbl_nfs::{Exports, Vfs};

/// How many components of a track's path make the folder allowed as a
/// whole: `/Volumes/<disk>/<folder>`, `/Users/<name>/<folder>`.
const ROOT_DEPTH: usize = 3;

/// The exports for a library: the folders its tracks live in, allowed as
/// wholes, and the few tracks too shallow for that by name.
pub fn exports(library: &Library) -> Exports {
    let mut exports = Exports::new();
    let mut trees: Vec<Vfs> = Vec::new();
    for row in 0..library.len() {
        let path = library.folder_path.get(row);
        let Some((export, relative)) = split(path) else {
            continue;
        };
        let tree = trees
            .iter()
            .position(|t| t.export_name() == export)
            .unwrap_or_else(|| {
                trees.push(Vfs::new(export));
                trees.len() - 1
            });
        let Some(tree) = trees.get_mut(tree) else {
            continue;
        };
        match root_of(path, &relative) {
            Some((folder, host)) => tree.allow_folder(folder, host),
            None => {
                tree.add_file_unsized(&relative, path);
            }
        }
    }
    for tree in trees {
        exports.insert(tree);
    }
    exports
}

/// The folder a track's path is allowed under, as its place in the export
/// and on the host; `None` for a path too shallow to have one.
///
/// `relative` is `path` without its drive or leading slash, separators
/// normalised, and the same length in bytes, so the host prefix is what
/// `path` has before it.
fn root_of<'a>(path: &'a str, relative: &'a str) -> Option<(&'a str, String)> {
    let mut end = 0;
    let mut components = 0;
    for (at, part) in relative
        .split('/')
        .map(|part| (part.as_ptr() as usize - relative.as_ptr() as usize, part))
    {
        if part.is_empty() {
            continue;
        }
        components += 1;
        end = at + part.len();
        if components == ROOT_DEPTH {
            break;
        }
    }
    // The root must be a folder above the file: three components of the
    // path, and a fourth for the file itself.
    if components < ROOT_DEPTH || relative.get(end..)?.trim_start_matches('/').is_empty() {
        return None;
    }
    let folder = relative.get(..end)?;
    let prefix = path.get(..path.len() - relative.len())?;
    Some((folder, format!("{prefix}{folder}")))
}

/// The export a path belongs to, and the path within it: rekordbox's
/// convention, as `rbl-nfs` states it.
pub fn split(path: &str) -> Option<(String, String)> {
    rbl_nfs::split_export(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_index::testing::{library_from, TestTrack};

    #[test]
    fn paths_split_into_an_export_and_a_relative_path() {
        assert_eq!(
            split("/Users/me/Music/a.mp3"),
            Some(("/".into(), "Users/me/Music/a.mp3".into()))
        );
        assert_eq!(
            split("C:/Users/me/a.mp3"),
            Some(("/C/".into(), "Users/me/a.mp3".into()))
        );
        assert_eq!(
            split(r"d:\Music\b.flac"),
            Some(("/D/".into(), "Music/b.flac".into()))
        );
        assert_eq!(split("relative/path.mp3"), None);
        assert_eq!(split(""), None);
    }

    #[test]
    fn a_track_is_allowed_under_its_first_three_folders() {
        let root = |path: &str| {
            let (_, relative) = split(path).unwrap();
            root_of(path, &relative).map(|(folder, host)| (folder.to_owned(), host))
        };
        assert_eq!(
            root("/Volumes/SD/RB/Artist/Album/one.mp3"),
            Some(("Volumes/SD/RB".into(), "/Volumes/SD/RB".into()))
        );
        assert_eq!(
            root("/Users/me/Music/one.mp3"),
            Some(("Users/me/Music".into(), "/Users/me/Music".into()))
        );
        assert_eq!(
            root(r"C:\Users\me\Music\one.mp3"),
            Some(("Users/me/Music".into(), r"C:\Users/me/Music".into()))
        );
        // Too shallow: the file would be the third component, or there is
        // nothing under the third.
        assert_eq!(root("/Users/me/one.mp3"), None);
        assert_eq!(root("/one.mp3"), None);
        assert_eq!(root("/Users/me/Music/"), None);
    }

    /// #282 / #40: a CDJ-3000X got E-8306 for every track. [OBS] In the
    /// capture on #43 the database server sent
    /// `/Volumes/SD/Music/PioneerDJ/…` and `/Volumes/Transcend/Music/…`,
    /// and the player's LOOKUPs went `Volumes` ok, `SD` ok, then `Music`
    /// NOENT and `Transcend` NOENT. A track directly on the card
    /// (`/Volumes/SD/<file>`) registered `Volumes` and `SD` as complete
    /// directories holding only the way to that file, which hid every
    /// allowed folder beside it. rekordbox stats every name on the host
    /// ([OBS, static] libFilSiNE LOOKUP), so a track's path resolves whatever
    /// other tracks share its folders.
    #[test]
    fn a_shallow_track_does_not_hide_the_library_folders_beside_it() {
        let sd = "/Volumes/SD/Music/PioneerDJ/Moved/one.mp3";
        let transcend = "/Volumes/Transcend/Music/Ron/two.mp3";
        let loose = "/Volumes/SD/loose.mp3";
        let track = |id, path: &'static str| TestTrack {
            id,
            title: "t",
            path,
            ..TestTrack::default()
        };
        for order in [
            [loose, sd, transcend],
            [sd, loose, transcend],
            [sd, transcend, loose],
        ] {
            let library = library_from(&[
                track(1, order[0]),
                track(2, order[1]),
                track(3, order[2]),
            ]);
            let exports = exports(&library);
            let root = exports.get("/").unwrap();
            // The allowed folders resolve, and are the host's.
            for (folder, host) in [
                ("Volumes/SD/Music", "/Volumes/SD/Music"),
                ("Volumes/Transcend/Music", "/Volumes/Transcend/Music"),
            ] {
                let node = root.resolve(folder);
                assert!(node.is_some(), "{folder} with {order:?}");
                assert_eq!(
                    root.source(node.unwrap()).unwrap().to_str(),
                    Some(host),
                    "{folder}"
                );
            }
            assert_eq!(
                root.source(root.resolve("Volumes/SD/loose.mp3").unwrap())
                    .unwrap()
                    .to_str(),
                Some(loose)
            );
            // Listings on the way show every way through, once.
            let names = |path: &str| -> Vec<String> {
                let at = root.resolve(path).unwrap();
                let mut names: Vec<String> = root
                    .children(at)
                    .iter()
                    .map(|&i| root.name(i).unwrap())
                    .collect();
                names.sort();
                names
            };
            assert_eq!(names("Volumes"), ["SD", "Transcend"]);
            assert_eq!(names("Volumes/SD"), ["Music", "loose.mp3"]);
            // Still nothing outside the library.
            assert_eq!(root.resolve("Volumes/Other"), None);
            assert_eq!(root.resolve("Volumes/SD/Other"), None);
        }
    }

    #[test]
    fn the_tree_reaches_the_library_folders_and_shallow_tracks_only() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        std::fs::create_dir_all(music.join("A")).unwrap();
        std::fs::write(music.join("A/one.mp3"), b"one").unwrap();
        std::fs::write(music.join("A/two.mp3"), b"two").unwrap();
        std::fs::write(
            music.join("stray.wav"),
            b"not in the library, but in its folder",
        )
        .unwrap();
        // Test tracks carry static paths; the temp dir's are leaked for the test.
        let leak = |path: std::path::PathBuf| -> &'static str {
            Box::leak(path.to_str().unwrap().to_owned().into_boxed_str())
        };
        let one = leak(music.join("A/one.mp3"));
        let two = leak(music.join("A/two.mp3"));
        let track = |id, path: &'static str| TestTrack {
            id,
            title: "t",
            path,
            ..TestTrack::default()
        };
        let library = library_from(&[
            track(1, one),
            track(2, two),
            track(3, "/Users/me/three.mp3"),
            track(4, "not absolute.mp3"),
        ]);
        let exports = exports(&library);
        assert_eq!(exports.names(), ["/"]);
        let root = exports.get("/").unwrap();
        let (_, relative) = split(one).unwrap();
        let (folder, _) = root_of(one, &relative).unwrap();

        // Nothing is built until asked: the tree is the root alone.
        assert_eq!(
            root.len(),
            1 + 3,
            "the shallow track's two directories and itself"
        );
        // The library's folder resolves, and what the host has in it.
        let (a_dir, _) = relative.rsplit_once('/').unwrap();
        assert!(a_dir.starts_with(folder));
        let a = root.resolve(a_dir).unwrap();
        assert_eq!(root.children(a).len(), 2);
        assert_eq!(
            root.source(root.resolve(&relative).unwrap())
                .unwrap()
                .to_str(),
            Some(one)
        );
        assert!(
            root.resolve(&format!("{}/stray.wav", a_dir.rsplit_once('/').unwrap().0))
                .is_some(),
            "a file in the folder is reachable, as with rekordbox"
        );
        // Outside every allowed folder is not, however real the file is.
        assert_eq!(root.resolve("etc/hosts"), None);
        assert_eq!(root.resolve("Users/me/Music/other.mp3"), None);
        // The folders on the way list only the way.
        let top: Vec<String> = root
            .children(root.root())
            .iter()
            .map(|&i| root.name(i).unwrap())
            .collect();
        assert!(top.contains(&"Users".to_owned()), "{top:?}");
        assert_eq!(
            root.source(root.resolve("Users/me/three.mp3").unwrap())
                .unwrap()
                .to_str(),
            Some("/Users/me/three.mp3")
        );
    }
}
