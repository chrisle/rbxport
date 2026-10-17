/**
 * The My Tag configuration window: rekordbox's panel beside the track list,
 * opened from the My Tag box at the top of the icon column.
 *
 * One list, top to bottom: each category as a heading row, then its tags,
 * each with a tick box. Taken from rekordbox 7.2.14 on Windows [OBS Winrig
 * chris-win11 2026-10-17, issue #539, `rekordbox-05`..`rekordbox-13` in the
 * parity folder] and 7.2.19's `browse::BrowseMyTagView` [static, arm64]:
 *
 * - A tick puts the tag on every selected track, an untick takes it off
 *   them all (`onCheckButton`). A tag is ticked only when every selected
 *   track has it. With nothing selected the boxes are greyed, and so are
 *   the names (`setCheckBoxEnable`).
 * - The ⊕ at the right of a category's last row, or of its heading while
 *   it has no tags, is Adding My Tag ("Create a new My Tag."). It adds
 *   "My Comment" (or the next free "My Comment (n)") at the end and opens
 *   its name for editing (`onAddButton`). It is greyed once the category
 *   holds 50.
 * - A click on a name, a category's included, opens it for editing
 *   (`listBoxItemClicked` -> `beginTextEdit`); Return or leaving the box
 *   saves it, Escape does not. An emptied name changes nothing; a name the
 *   category already has is refused with "An item with the same name
 *   already exists!" (`didChangeLabelText`). Names stop at 256 characters
 *   (`labelTextChanged`).
 * - Right-clicking a tag offers one entry, Delete, which deletes it at once
 *   (`deleteItem`, no confirmation). A category heading has no menu: its
 *   categories cannot be deleted.
 *
 * Not rekordbox's: a file the Explorer lists that is not in the collection
 * leaves the boxes greyed here, where rekordbox would import it first
 * (`importCheckBeforeAddMyTag`); every other edit in this app refuses such
 * a file the same way.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { ContextMenu } from "@/components/ContextMenu";
import { getBackend } from "@/ipc/client";
import type { Backend, TrackLookups } from "@/ipc/types";
import { isLooseId } from "@/lib/explorer";
import { MY_TAG_LIMIT } from "@/lib/myTags";
import { useTooltip } from "@/store/usePreferences";
import styles from "./MyTagPanel.module.css";

/** rekordbox cuts a typed name to this many characters (`labelTextChanged`). */
const NAME_LIMIT = 256;

type Categories = TrackLookups["myTagCategories"];

export interface MyTagPanelProps {
  /** Where the shell puts it, since the shell's grid decides that. */
  className?: string | undefined;
  /** The browser's selected tracks, as the information panel follows them. */
  selection: readonly string[];
  /** rekordbox holds the library, or Library Protection is on. */
  readOnly: boolean;
  /** Bumped by every library change: the tags and ticks are read again. */
  libraryGeneration: number;
  /** Runs one write and reports it, or why it was refused. */
  onEdit: (what: string, edit: (b: Backend) => Promise<unknown>) => Promise<void>;
}

export function MyTagPanel({ className, selection, readOnly, libraryGeneration, onEdit }: MyTagPanelProps) {
  const tip = useTooltip();
  const [categories, setCategories] = useState<Categories>([]);
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  /** The id whose name is open for editing. */
  const [editing, setEditing] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; tag: string } | null>(null);

  useEffect(() => {
    let live = true;
    void getBackend()
      .then((b) => b.trackLookups())
      .then((l) => { if (live) setCategories(l.myTagCategories); })
      .catch(() => { if (live) setCategories([]); });
    return () => { live = false; };
  }, [libraryGeneration]);

  const tracks = selection.filter((id) => !isLooseId(id));
  const tickable = !readOnly && selection.length > 0 && tracks.length === selection.length;
  const selectionKey = tracks.join("\n");
  useEffect(() => {
    let live = true;
    if (selectionKey === "") {
      setTicked(new Set());
      return;
    }
    void getBackend()
      .then((b) => b.selectionMyTags(selectionKey.split("\n")))
      .then((ids) => { if (live) setTicked(new Set(ids)); })
      .catch(() => { if (live) setTicked(new Set()); });
    return () => { live = false; };
  }, [selectionKey, libraryGeneration]);

  const tick = useCallback(
    (tag: string, on: boolean) => {
      if (!tickable) return;
      // Shown at once; the reload that follows the write confirms it.
      setTicked((current) => {
        const next = new Set(current);
        if (on) next.add(tag); else next.delete(tag);
        return next;
      });
      void onEdit(on ? "My Tag added." : "My Tag removed.", (b) => b.edits.putMyTag(tracks, tag, on));
    },
    [tickable, tracks, onEdit],
  );

  const add = useCallback(
    (category: string) => {
      if (readOnly) return;
      void onEdit("My Tag created.", async (b) => {
        const made = await b.edits.createMyTag(category);
        setEditing(made.id);
      });
    },
    [readOnly, onEdit],
  );

  const rename = useCallback(
    (id: string, current: string, name: string) => {
      setEditing(null);
      // An emptied box, or the name it had, writes nothing.
      if (name === "" || name === current) return;
      void onEdit("My Tag renamed.", (b) => b.edits.renameMyTag(id, name));
    },
    [onEdit],
  );

  const remove = useCallback(
    (id: string) => { void onEdit("My Tag deleted.", (b) => b.edits.deleteMyTag(id)); },
    [onEdit],
  );

  const addButton = (category: Categories[number]) => {
    const full = category.tags.length >= MY_TAG_LIMIT;
    return (
      <button
        type="button"
        className={styles.add}
        title={tip("Create a new My Tag.")}
        aria-label="Create a new My Tag."
        disabled={readOnly || full}
        onClick={(event) => { event.stopPropagation(); add(category.id); }}
      >
        <AddIcon />
      </button>
    );
  };

  return (
    <section
      className={className ? `${styles.panel} ${className}` : styles.panel}
      aria-label="My Tag"
      data-enabled={tickable || undefined}
    >
      <div className={styles.list} role="list">
        {categories.map((category) => (
          <div key={category.id} role="group" aria-label={category.name}>
            <div
              className={styles.heading}
              role="listitem"
              onClick={() => { if (!readOnly) setEditing(category.id); }}
            >
              <Name
                value={category.name}
                editing={editing === category.id}
                onDone={(name) => rename(category.id, category.name, name)}
                onCancel={() => setEditing(null)}
              />
              {category.tags.length === 0 ? addButton(category) : null}
            </div>
            {category.tags.map((tag, index) => {
              const on = ticked.has(tag.id);
              return (
                <div
                  key={tag.id}
                  className={styles.tag}
                  role="listitem"
                  onClick={() => { if (!readOnly) setEditing(tag.id); }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    if (!readOnly) setMenu({ x: event.clientX, y: event.clientY, tag: tag.id });
                  }}
                >
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={on}
                    aria-label={tag.name}
                    className={styles.check}
                    data-checked={on || undefined}
                    disabled={!tickable}
                    onClick={(event) => { event.stopPropagation(); tick(tag.id, !on); }}
                  >
                    {on ? <MyTagTick /> : null}
                  </button>
                  <Name
                    value={tag.name}
                    editing={editing === tag.id}
                    onDone={(name) => rename(tag.id, tag.name, name)}
                    onCancel={() => setEditing(null)}
                  />
                  {index === category.tags.length - 1 ? addButton(category) : null}
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label="My Tag"
          context={{ inPlaylist: false, hasFile: false, readOnly }}
          rows={[{ label: "Delete", action: "delete" as const }]}
          onClose={() => setMenu(null)}
          onChoose={() => { remove(menu.tag); setMenu(null); }}
        />
      ) : null}
    </section>
  );
}

/** A row's name, or the box it is typed into while it is being renamed. */
function Name({ value, editing, onDone, onCancel }: {
  value: string;
  editing: boolean;
  onDone: (name: string) => void;
  onCancel: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  // Escape is a cancel, and the blur that follows it must not save.
  const cancelled = useRef(false);
  useEffect(() => {
    if (!editing) return;
    cancelled.current = false;
    input.current?.focus();
    input.current?.select();
  }, [editing]);
  if (!editing) return <span className={styles.name}>{value}</span>;
  return (
    <input
      ref={input}
      className={styles.edit}
      defaultValue={value}
      maxLength={NAME_LIMIT}
      aria-label="Name"
      spellCheck={false}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        } else if (event.key === "Escape") {
          event.preventDefault();
          cancelled.current = true;
          onCancel();
        }
      }}
      onBlur={(event) => {
        if (cancelled.current) return;
        onDone(event.currentTarget.value);
      }}
    />
  );
}

/**
 * The tick of a ticked box: `skins/checkbox_on.svg`'s path (a 10px box), framed
 * so it lands where drawTickBox puts `checkbox_mark.png`'s tick once it is
 * stretched from 16px to 14px: svg unit u sits at 1.225u - 0.125px in the
 * 12px face, so the face shows units 0.102..9.898.
 */
function MyTagTick() {
  return (
    <svg className={styles.tick} viewBox="0.102 0.102 9.796 9.796" aria-hidden focusable="false">
      <path d="M2 6.051V4.722l2.038 1.78L8 2.001v1.57L4.041 7.998z" fill="currentColor" />
    </svg>
  );
}

/** rekordbox's `skins/brws_tree_addIcon.svg`, the ⊕ of Adding My Tag. */
function AddIcon() {
  return (
    <svg className={styles.addIcon} viewBox="0 0 11 11" aria-hidden focusable="false">
      <circle cx="5.5" cy="5.5" r="5.5" fill="currentColor" />
      <path d="M9 6H6v3H5V6H2V5h3V2h1v3h3z" className={styles.addPlus} />
    </svg>
  );
}
