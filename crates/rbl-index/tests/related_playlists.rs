#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_index::testing::{add_folder, add_playlist, add_smart_playlist, library_from, TestTrack};

fn tracks() -> Vec<TestTrack> {
    vec![
        TestTrack {
            id: 1,
            title: "Alpha",
            artist: "Daft Punk",
            album: "Discovery",
            genre: "House",
            bpm_x100: 12_800,
            path: "/m/alpha.mp3",
            ..TestTrack::default()
        },
        TestTrack {
            id: 2,
            title: "Beta",
            artist: "Bicep",
            album: "Isles",
            genre: "Electronica",
            bpm_x100: 12_000,
            path: "/m/beta.mp3",
            ..TestTrack::default()
        },
    ]
}

#[test]
fn related_playlists_lists_ordinary_and_smart_containers() {
    let mut lib = library_from(&tracks());
    add_playlist(&mut lib, "Club", &[0]);
    let rule = "<NODE Id=\"1\" LogicalOperator=\"1\" AutomaticUpdate=\"1\"><CONDITION PropertyName=\"genre\" Operator=\"8\" ValueUnit=\"\" ValueLeft=\"house\" ValueRight=\"\"/></NODE>";
    add_smart_playlist(&mut lib, "House", rule);

    assert_eq!(lib.playlists_containing(0), vec![0, 1]);
    assert!(lib.playlists_containing(1).is_empty());
}

#[test]
fn a_folder_is_not_a_playlist_a_track_is_in() {
    let mut lib = library_from(&tracks());
    let folder = add_folder(&mut lib, "Gigs");
    // The builders top the tree, so the nesting is set by hand here.
    let nested = add_playlist(&mut lib, "Friday", &[0]);
    let mut playlists = (*lib.playlists()).clone();
    playlists.parent[nested] = folder as u32;
    lib.set_playlists(playlists);

    // The folder holds playlists, not tracks; the playlist under it holds one.
    assert_eq!(lib.playlists_containing(0), vec![nested]);
    assert!(lib.playlists_containing(1).is_empty());
}
