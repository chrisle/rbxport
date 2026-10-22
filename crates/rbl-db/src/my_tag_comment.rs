//! rekordbox's Preferences > Advanced > Browse > My Tag > Add "My Tag" to
//! the "Comments" (#605): a track's My Tags written at the end of its
//! comment, as ` /* Tag / Tag */`.
//!
//! Read from rekordbox 7.2.x (macOS arm64, static analysis; the
//! disassembly is kept in the #605 parity folder):
//!
//! - The setting is `writeMytag`, off by default
//!   (`SettingIF::getCurrentWriteMytag` @0x1015c01e8).
//! - The block starts at the first ` /* ` in the comment. Whatever stands
//!   before it is kept as it is; the block, and anything after it, is
//!   replaced. A comment with no ` /* ` is kept whole and the block is
//!   added after it.
//! - The first tag is joined with ` /* `, the others with ` / `, and the
//!   block is closed with ` */` once it holds a tag. A tag is added only
//!   while the comment stays within 253 characters; one that does not fit
//!   is passed over and a later, shorter one may still go in.
//! - While the setting is on, a tick or untick in the My Tag panel, and a
//!   tag renamed, deleted or moved, rewrites the comment of each selected
//!   track, with the track's tags in the order they were put on it
//!   (`BrowseListViewer::updateCommentWithMyTag` @0x10079e338).
//! - Turning the setting on rewrites every track that has a My Tag, with
//!   its tags in the My Tag panel's order and no block at all for a
//!   comment already longer than 248 characters before it; turning it off
//!   takes the block off every such track (`WriteOrEraseMyTagWindow::run`
//!   @0x101fe92a4). A track with no My Tag is left as it is either way.
//!
//! Lengths are counted in characters, as `juce::String::length` counts
//! them.

/// What opens the block, and how rekordbox finds an earlier one.
pub const OPEN: &str = " /* ";
/// What goes between two tags.
pub const SEPARATOR: &str = " / ";
/// What closes the block.
pub const CLOSE: &str = " */";

/// The longest comment a tag is added to: one that would pass this is
/// passed over (`cmp w8, #0xfd`).
const LIMIT: usize = 253;
/// Turning the setting on writes no block into a comment longer than this
/// (`cmp w0, #0xf8` in `WriteOrEraseMyTagWindow::run`).
const BULK_BASE_LIMIT: usize = 248;

fn length(text: &str) -> usize {
    text.chars().count()
}

/// The comment as it stands without its My Tag block: everything before
/// the first ` /* `, or the whole comment when there is none.
#[must_use]
pub fn without_block(comment: &str) -> &str {
    comment.find(OPEN).map_or(comment, |at| &comment[..at])
}

/// `base` with `names` added as rekordbox adds them, each only while the
/// result stays within the limit, and the block closed when it holds one.
fn append<'a>(base: &str, names: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = base.to_owned();
    for name in names {
        let joint = if out.contains(OPEN) { SEPARATOR } else { OPEN };
        let piece = format!("{joint}{name}");
        if length(&out) + length(&piece) <= LIMIT {
            out.push_str(&piece);
        }
    }
    if out.contains(OPEN) {
        out.push_str(CLOSE);
    }
    out
}

/// The comment an edit leaves while the setting is on: `comment` without
/// its old block, then `names` (the track's tags, in the order they were
/// put on it). A name that is empty, as for a tag no longer in the panel,
/// is passed over, as `updateCommentWithMyTag` passes it over.
#[must_use]
pub fn refreshed<'a>(comment: &str, names: impl IntoIterator<Item = &'a str>) -> String {
    append(without_block(comment), names.into_iter().filter(|name| !name.is_empty()))
}

/// The comment turning the setting on leaves: `names` are the track's
/// tags in the My Tag panel's order. A comment already longer than 248
/// characters without its block gets none.
#[must_use]
pub fn written<'a>(comment: &str, names: impl IntoIterator<Item = &'a str>) -> String {
    let base = without_block(comment);
    if length(base) > BULK_BASE_LIMIT {
        return base.to_owned();
    }
    append(base, names)
}

/// The comment turning the setting off leaves: the comment without its
/// block.
#[must_use]
pub fn erased(comment: &str) -> &str {
    without_block(comment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_go_after_the_comment_in_one_block() {
        assert_eq!(refreshed("Visit bandcamp", ["Techno", "Dark"]), "Visit bandcamp /* Techno / Dark */");
        // An empty comment keeps the space before the block, as rekordbox's
        // ` /* ` is added to an empty string.
        assert_eq!(refreshed("", ["IDM", "Breaks", "Build down"]), " /* IDM / Breaks / Build down */");
    }

    #[test]
    fn the_old_block_and_whatever_follows_it_are_replaced() {
        assert_eq!(refreshed("Keep this /* Old / Tags */", ["New"]), "Keep this /* New */");
        assert_eq!(refreshed("Keep /* Old */ typed after", ["New"]), "Keep /* New */");
        // Only the first ` /* ` counts.
        assert_eq!(refreshed("A /* B /* C */", ["T"]), "A /* T */");
        // `/*` without the spaces around it is not a block.
        assert_eq!(refreshed("a/*b*/", ["T"]), "a/*b*/ /* T */");
    }

    #[test]
    fn no_tags_take_the_block_off_and_leave_the_rest() {
        assert_eq!(refreshed("Keep this /* Old */", Vec::<&str>::new()), "Keep this");
        assert_eq!(refreshed("Plain comment", Vec::<&str>::new()), "Plain comment");
        assert_eq!(refreshed("", Vec::<&str>::new()), "");
    }

    #[test]
    fn empty_names_are_passed_over_when_editing() {
        assert_eq!(refreshed("c", ["", "A", ""]), "c /* A */");
        assert_eq!(refreshed("c", [""]), "c");
    }

    #[test]
    fn a_tag_that_would_pass_253_characters_is_passed_over_and_a_shorter_one_still_fits() {
        let base = "x".repeat(240);
        // 240 + " /* " (4) + 9 = 253: fits exactly.
        let fits = refreshed(&base, ["123456789"]);
        assert_eq!(fits, format!("{base} /* 123456789 */"));
        // 240 + 4 + 10 = 254: passed over; the next one still goes in.
        let skipped = refreshed(&base, ["1234567890", "abc"]);
        assert_eq!(skipped, format!("{base} /* abc */"));
        // Nothing fits: no block at all.
        let none = refreshed(&"y".repeat(252), ["A"]);
        assert_eq!(none, "y".repeat(252));
    }

    #[test]
    fn lengths_are_characters_not_bytes() {
        let base = "é".repeat(240);
        assert_eq!(refreshed(&base, ["123456789"]), format!("{base} /* 123456789 */"));
    }

    #[test]
    fn turning_it_on_writes_no_block_past_248_characters() {
        let long = "z".repeat(249);
        assert_eq!(written(&long, ["A"]), long);
        let edge = "z".repeat(248);
        assert_eq!(written(&edge, ["A"]), format!("{edge} /* A */"));
        // The edit path has no such check, but 249 + " /* " + a name is
        // already past 253, so the two agree.
        assert_eq!(refreshed(&long, ["A"]), long);
        assert_eq!(written(&format!("{long} /* Old */"), ["A"]), long);
    }

    #[test]
    fn turning_it_off_keeps_only_what_stands_before_the_block() {
        assert_eq!(erased("Keep /* A / B */"), "Keep");
        assert_eq!(erased("No block"), "No block");
        assert_eq!(erased(" /* A */"), "");
    }
}
