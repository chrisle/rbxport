/**
 * The rail beside the tree: which part of the library the tree is showing.
 *
 * rekordbox calls this the tree shortcut column (`browseSetting.xml`,
 * `TreeShortcut`, width 57). Selecting one jumps the tree to that section; it
 * is a shortcut rather than a filter, so nothing is hidden.
 *
 * No Collection button: All Tracks is the first row of the tree and never out
 * of sight, so jumping to it is jumping to where you already are.
 */
import { useState, type CSSProperties, type SVGProps } from "react";

import type { Source } from "@/lib/tree";

import { ContextMenu } from "@/components/ContextMenu";
import { DeviceIcon, ExplorerIcon, FolderIcon, HistoryIcon, ListIcon, SyncIcon } from "@/components/icons";
import { shortcutMenu } from "@/lib/contextMenus";
import { BROWSE_ICON_SIZES } from "@/lib/preferences";
import { usePreferences, useTooltip } from "@/store/usePreferences";
import styles from "./SourceRail.module.css";

const SOURCES: ReadonlyArray<{
  id: Source;
  label: string;
  Icon: (props: SVGProps<SVGSVGElement>) => React.ReactElement;
}> = [
  // rekordbox 7.2.11's rail, read off its tooltips on 2026-09-18 [OBS]:
  // Collection, Playlists, Related Track List, Hot Cue Bank List, Apple
  // Music, Beatport, Inflyte, Explorer, Devices, Cloud Export, Histories,
  // Recordings. The streaming and cloud buttons are not here (no such
  // services behind this app), Hot Cue Bank List and Recordings are not
  // built, and Collection is All Tracks at the top of the tree; the rest
  // keep rekordbox's order. Related Tracks is hidden until finished.
  { id: "playlists", label: "Playlists", Icon: FolderIcon },
  { id: "explorer", label: "Explorer", Icon: ExplorerIcon },
  { id: "devices", label: "Devices", Icon: DeviceIcon },
  { id: "histories", label: "Histories", Icon: HistoryIcon },
  // Ours: rekordbox 7.2.11 shows no Tag List on its rail, its sub-browser
  // rail (Collection, Playlists, Hot Cue Bank List, Devices, Histories,
  // Recordings) or its right-hand icon column (My Tag, Related Tracks,
  // Track Suggestion, Information, Sub-browser); where it shows the list
  // its Add To Tag List fills is not known. The icon is the playlist's.
  { id: "tagList", label: "Tag List", Icon: ListIcon },
];

export interface SourceRailProps {
  selected: Source;
  onSelect: (source: Source) => void;
  /** Sources with nothing in them, shown but dimmed rather than hidden. */
  empty?: ReadonlySet<Source>;
  /**
   * Opens the Sync Manager. rekordbox keeps its ⟳ at the foot of this rail,
   * away from the sections above, and so does this; it is the only way in,
   * there too — no menu item and no key opens it (german.lang has "Launch
   * Sync Manager" as the button's tip and nothing for a menu).
   */
  onOpenSync?: (() => void) | undefined;
  /**
   * Add To Shortcut's playlists and folders, under the sections. rekordbox
   * 7.2.11 keeps them in a column of their own beside this rail, opened
   * and closed by an arrow above it ("Open/close shortcuts"), five slots
   * then Collection, iTunes and Devices, each drawn as the node's icon and
   * its name cut to the width [OBS 2026-09-18,
   * docs/screenshots/shortcuts-column-7.2.11@2x]. This app has no second
   * column: the shortcuts sit on this rail under the sections, drawn the
   * same way, and a right click on one offers Delete Shortcut as there.
   */
  shortcuts?: readonly { id: string; name: string; selected: boolean }[] | undefined;
  onOpenShortcut?: ((id: string) => void) | undefined;
  onDeleteShortcut?: ((id: string) => void) | undefined;
}

export function SourceRail({
  selected, onSelect, empty, onOpenSync, shortcuts = [], onOpenShortcut, onDeleteShortcut,
}: SourceRailProps) {
  const tip = useTooltip();
  const iconSize = usePreferences().view.browseIconSize;
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  return (
    <div
      className={styles.rail}
      data-icon-only={iconSize === BROWSE_ICON_SIZES[0] || undefined}
      style={{ "--rail-icon-size": `${iconSize}px` } as CSSProperties}
    >
      <div className={styles.sources} role="tablist" aria-label="Library sources" aria-orientation="vertical">
        {SOURCES.map(({ id, label, Icon }) => (
          <button
            key={id}
            type="button"
            className={styles.button}
            role="tab"
            aria-selected={selected === id}
            // Dimmed rather than removed: a missing Devices button reads as a
            // broken app, an empty one reads as no device plugged in.
            data-empty={empty?.has(id) || undefined}
            title={tip(label)}
            aria-label={label}
            onClick={() => onSelect(id)}
          >
            <Icon className={styles.icon} />
            <span className={styles.label}>{label}</span>
          </button>
        ))}
      </div>
      {shortcuts.length > 0 ? (
        <div className={styles.sources} role="list" aria-label="Shortcuts">
          {shortcuts.map((shortcut) => (
            <button
              key={shortcut.id}
              type="button"
              className={styles.button}
              role="listitem"
              aria-selected={shortcut.selected}
              title={tip(shortcut.name)}
              aria-label={shortcut.name}
              onClick={() => onOpenShortcut?.(shortcut.id)}
              onContextMenu={(e) => {
                if (!onDeleteShortcut) return;
                e.preventDefault();
                setMenu({ id: shortcut.id, x: e.clientX, y: e.clientY });
              }}
            >
              <ListIcon className={styles.icon} />
              <span className={styles.label}>{shortcut.name}</span>
            </button>
          ))}
        </div>
      ) : null}
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          rows={shortcutMenu()}
          label="Shortcut"
          context={{ inPlaylist: false, hasFile: false, readOnly: false }}
          onChoose={() => {
            onDeleteShortcut?.(menu.id);
            setMenu(null);
          }}
          onClose={() => setMenu(null)}
        />
      ) : null}
      {onOpenSync ? (
        <button
          type="button"
          className={styles.sync}
          title={tip("Launch Sync Manager")}
          aria-label="Sync Manager"
          onClick={onOpenSync}
        >
          <SyncIcon className={styles.icon} />
          <span className={styles.label}>Sync</span>
        </button>
      ) : null}
    </div>
  );
}
