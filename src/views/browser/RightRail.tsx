/**
 * The icon column at the right edge of the browser.
 *
 * Three outlined boxes: My Tag, Information and Sub-Browser. rekordbox
 * draws five — Related Tracks and Track Suggestion between My Tag and
 * Information — and those two are left out of this column by request,
 * recorded in TODO.md under "Deliberate divergences": Related Tracks is
 * hidden until finished. My Tag opens rekordbox's My Tag configuration
 * window (issue #539), which takes the Information window's place, as
 * rekordbox's does: opening either closes the other [OBS rekordbox 7.2.14,
 * Winrig chris-win11 2026-10-17]. The boxes are vertically centered as a
 * group. The wording is german.lang's.
 */
import { InfoIcon, MyTagIcon, SubBrowseIcon } from "@/components/icons";
import styles from "./RightRail.module.css";
import { useTooltip } from "@/store/usePreferences";

export interface RightRailProps {
  /** Where the shell puts it, since the shell's grid decides that. */
  className?: string | undefined;
  myTagOpen: boolean;
  onToggleMyTag: () => void;
  infoOpen: boolean;
  onToggleInfo: () => void;
  subOpen: boolean;
  onToggleSub: () => void;
}

export function RightRail({
  className, myTagOpen, onToggleMyTag, infoOpen, onToggleInfo, subOpen, onToggleSub,
}: RightRailProps) {
  const tip = useTooltip();
  return (
    <div className={className ? `${styles.rail} ${className}` : styles.rail} role="toolbar" aria-label="Browser panels" aria-orientation="vertical">
      <div className={styles.group}>
        <button
          type="button"
          className={styles.button}
          title={tip("Display My Tag configuration window.")}
          aria-label="My Tag"
          aria-pressed={myTagOpen}
          onClick={onToggleMyTag}
        >
          <MyTagIcon className={styles.icon} />
        </button>
        <button
          type="button"
          className={styles.button}
          title={tip("Display information window.")}
          aria-label="Information"
          aria-pressed={infoOpen}
          onClick={onToggleInfo}
        >
          <InfoIcon className={styles.icon} />
        </button>
        <button
          type="button"
          className={styles.button}
          title={tip("Display sub-browser window.")}
          aria-label="Sub-Browser Window"
          aria-pressed={subOpen}
          onClick={onToggleSub}
        >
          <SubBrowseIcon className={styles.icon} />
        </button>
      </div>
    </div>
  );
}
