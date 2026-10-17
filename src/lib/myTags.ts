/**
 * rekordbox's My Tag rules that the window needs as well as the backend.
 *
 * The backend's writer (`rbl_db::write`) keeps the same two, from the same
 * static analysis of rekordbox 7.2.19 for macOS (arm64).
 */

/**
 * The most tags one category holds. rekordbox greys a category's `+` at
 * this many: `BrowseMyTagView`'s constructor stores 50, and
 * `MyTagItemComponent::isActiveButton` @0x10066c940 compares the
 * category's tag count with it.
 */
export const MY_TAG_LIMIT = 50;

/**
 * The name Adding My Tag gives a new tag: "My Comment", or "My Comment (n)"
 * with the smallest n from 1 that no tag in the category already has,
 * compared exactly (`BrowseMyTagView::createNewItemName` @0x10066e770).
 */
export function newMyTagName(existing: readonly string[]): string {
  const base = "My Comment";
  let name = base;
  for (let n = 1; existing.includes(name); n += 1) name = `${base} (${n})`;
  return name;
}
