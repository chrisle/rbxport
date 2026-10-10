/**
 * Interface icons bundled with the application.
 *
 * Every icon paints in `currentColor`, so colour comes from the CSS token on
 * whatever contains it rather than from the icon.
 */
import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

export const ArtworkDeleteIcon = (props: IconProps) => (
  <svg viewBox="0 0 20 22" aria-hidden focusable="false" {...props}>
    <path d="M10 10 6 5h2.6V0h2.8v5H14z" fill="currentColor"/> <rect x="4" y="11" width="12" height="1.6" fill="currentColor"/> <path fillRule="evenodd" fill="currentColor" d="M5.5 13.6h9l-.8 8.4H6.3zM7.6 15v5.5h.8V15zm2 0v5.5h.8V15zm2 0v5.5h.8V15z"/>
  </svg>
);

export const ArtworkImportIcon = (props: IconProps) => (
  <svg viewBox="0 0 20 22" aria-hidden focusable="false" {...props}>
    <path d="M10 1 14 6h-2.6v5h-2.8V6H6z" fill="currentColor"/> <path d="M1 12h6l1.6 1.6H19V21H1z" fill="currentColor"/>
  </svg>
);

export const ClearCircleIcon = (props: IconProps) => (
  <svg viewBox="0 0 11 11" aria-hidden focusable="false" {...props}>
    <path fillRule="evenodd" fill="currentColor" d="M5.5 0a5.5 5.5 0 1 1 0 11 5.5 5.5 0 0 1 0-11Zm2.83 3.38-.71-.71L5.5 4.79 3.38 2.67l-.71.71L4.79 5.5 2.67 7.62l.71.71L5.5 6.21l2.12 2.12.71-.71L6.21 5.5Z"/>
  </svg>
);

export const CollectionIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <circle cx="8" cy="8" r="5.4" fill="none" stroke="currentColor" strokeWidth="1.4"/> <circle cx="8" cy="8" r="1.5" fill="currentColor"/>
  </svg>
);

/**
 * A saved loop on a cue row: rekordbox 7's `skins/cueLoopIcon.svg` (10x6, its
 * own fill `rgb(255, 130, 5)`), the orange mark the HOT CUE and MEMORY panels
 * put on a row that holds a loop.
 */
export const CueLoopIcon = (props: IconProps) => (
  <svg viewBox="0 0 10 6" aria-hidden focusable="false" {...props}>
    <path fillRule="evenodd" fill="currentColor" d="M8.057 0L4.216 0L5.032 1.404L8.057 1.404C8.367 1.404 8.629 1.672 8.629 1.99L8.629 4.002C8.629 4.32 8.367 4.588 8.057 4.588L3.651 4.588C3.341 4.588 3.079 4.32 3.079 4.002L3.079 3.875L4.734 3.875L2.367 0.065L0 3.875L1.709 3.875L1.709 4.002C1.709 5.097 2.583 5.992 3.651 5.992L8.057 5.992C9.126 5.992 10 5.097 10 4.002L10 1.99C10 0.895 9.126 0 8.057 0Z"/>
  </svg>
);

export const CommentIcon = (props: IconProps) => (
  <svg viewBox="0 0 12 10" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M2.2 0h7.6A2.2 2.2 0 0 1 12 2.2v2.6A2.2 2.2 0 0 1 9.8 7H6.1L2 10V7A2.2 2.2 0 0 1 0 4.8V2.2A2.2 2.2 0 0 1 2.2 0Z"/>
  </svg>
);

export const DeviceIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="4.2" y="1.6" width="7.6" height="9.4" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M6.4 11v2.2h3.2V11" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M6.6 4.2h2.8" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"/>
  </svg>
);

export const EjectIcon = (props: IconProps) => (
  <svg viewBox="0 0 26 29" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M13 0 26 18H0z"/> <rect fill="currentColor" x="0" y="23" width="26" height="6"/>
  </svg>
);

export const ExplorerIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="2.4" y="2.2" width="11.2" height="8.2" rx="1" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M0.4 11.7h15.2l-1.3 2.4H1.7z" fill="currentColor"/>
  </svg>
);

export const FilterIcon = (props: IconProps) => (
  <svg viewBox="0 0 11 11" aria-hidden focusable="false" {...props}>
    <rect x="0.5" y="0" width="2" height="11" fill="currentColor"/><rect x="4.5" y="0" width="6" height="11" fill="currentColor"/>
  </svg>
);

export const FolderIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 12" aria-hidden focusable="false" {...props}>
    <path d="M0 2h5l1.5 1.5H14V11H0z" fill="currentColor"/>
  </svg>
);

export const GearIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <g fill="currentColor"> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(0 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(45 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(90 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(135 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(180 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(225 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(270 8 8)"/> <rect x="6.9" y="0.6" width="2.2" height="3.2" transform="rotate(315 8 8)"/> <path fillRule="evenodd" d="M8 1.7A6.3 6.3 0 1 0 8 14.3 6.3 6.3 0 0 0 8 1.7zm0 2.7a3.6 3.6 0 1 1 0 7.2 3.6 3.6 0 0 1 0-7.2z"/> </g>
  </svg>
);

export const GridAlignAllIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect x="8.15" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="12.95" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="17.60" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="28.95" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="33.55" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="38.35" y="15" width="1.5" height="16" fill="currentColor"/> <rect className="dim" x="23" y="8.7" width="2" height="28.6" fill="currentColor"/> <polygon className="dim" points="21.3,8.7 26.7,8.7 24,13.2" fill="currentColor"/> <polygon className="dim" points="21.3,37.3 26.7,37.3 24,32.8" fill="currentColor"/>
  </svg>
);

export const GridAlignHereIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect className="faint" x="8.15" y="15" width="1.5" height="16" fill="currentColor"/> <rect className="faint" x="12.95" y="15" width="1.5" height="16" fill="currentColor"/> <rect className="faint" x="17.60" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="28.95" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="33.55" y="15" width="1.5" height="16" fill="currentColor"/> <rect x="38.35" y="15" width="1.5" height="16" fill="currentColor"/> <polygon className="dim" points="21.3,8.7 26.7,8.7 24,13.2" fill="currentColor"/> <polygon className="dim" points="21.3,37.3 26.7,37.3 24,32.8" fill="currentColor"/>
  </svg>
);

export const GridCutIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="21.5,10.2 30.8,10.2 32.9,12.3 32.9,13.3 22.4,13.3" fill="currentColor"/> <polygon points="29.7,13.3 32.9,13.3 33,20 33,34.1 30.3,34.1 30.3,20" fill="currentColor"/> <rect x="15" y="28.4" width="18" height="5.7" fill="currentColor"/> <polygon points="15.2,10.75 18.1,10.75 27.2,28.4 24.05,28.4" fill="currentColor"/> <path d="M15 19.1Q19.4 21.5 19.6 28.4L15 28.4Z" fill="currentColor"/>
  </svg>
);

export const GridDoubleIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect className="dim" x="6.35" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="11.25" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="16.15" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <polygon points="20.6,18.2 23.6,18.2 30.0,27.8 27.0,27.8" fill="currentColor"/> <polygon points="27.0,18.2 30.0,18.2 23.6,27.8 20.6,27.8" fill="currentColor"/> <path d="M34.25 19.06A4.2 4.2 0 1 1 42.2 21.9L34.6 27.4" fill="none" stroke="currentColor" strokeWidth="2.4"/> <rect x="33.7" y="26.7" width="10" height="1.9" fill="currentColor"/>
  </svg>
);

export const GridHalveIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect className="dim" x="4.91" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="9.59" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="14.36" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <polygon points="19.3,18.2 22.3,18.2 28.7,27.8 25.7,27.8" fill="currentColor"/> <polygon points="25.7,18.2 28.7,18.2 22.3,27.8 19.3,27.8" fill="currentColor"/> <polygon points="33.6,9 38.3,7.5 38.3,10 33.6,10" fill="currentColor"/> <rect x="36.9" y="7.5" width="1.4" height="10.6" fill="currentColor"/> <rect x="32" y="21.4" width="11" height="1.6" fill="currentColor"/> <path d="M33.5 31.7A3.4 3.4 0 1 1 39.7 32.5L34.1 37.9" fill="none" stroke="currentColor" strokeWidth="2.2"/> <rect x="32.1" y="37.3" width="9.4" height="1.9" fill="currentColor"/>
  </svg>
);

export const GridLockOpenIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <path fillRule="evenodd" d="M12 23H36V39H12ZM22 37V31.46A4 4 0 1 1 26 31.46V37Z" fill="currentColor"/> <path d="M14 17.2A10 10 0 0 1 34 17.2V23H30V17.2A6 6 0 0 0 18 17.2V19H14Z" fill="currentColor"/>
  </svg>
);

export const GridLockIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <path fillRule="evenodd" d="M12 23H36V39H12ZM22 37V31.46A4 4 0 1 1 26 31.46V37Z" fill="currentColor"/> <path d="M14 17.2A10 10 0 0 1 34 17.2V23H30V17.2A6 6 0 0 0 18 17.2V23H14Z" fill="currentColor"/>
  </svg>
);

export const GridMarkIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect x="22.4" y="10.3" width="3.2" height="12.7" fill="currentColor"/> <rect className="head" x="22.4" y="23" width="3.2" height="12.85" fill="currentColor"/>
  </svg>
);

export const GridMetronomeIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect x="15" y="18.2" width="3.1" height="7.9" fill="currentColor"/> <rect x="21.6" y="15" width="3.2" height="14.3" fill="currentColor"/> <rect x="28" y="11.9" width="3.4" height="22.2" fill="currentColor"/>
  </svg>
);

export const GridNarrowIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="6.5,18.5 6.5,27.5 14,23" fill="currentColor"/> <rect className="dim" x="19.25" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="24.15" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="28.95" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <polygon points="43,18.5 43,27.5 35.5,23" fill="currentColor"/>
  </svg>
);

export const GridRedoIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="31,16 23.5,11.8 23.5,20.5" fill="currentColor"/> <path d="M22.57 16.52A6.5 6.5 0 1 0 23.68 29.28" fill="none" stroke="currentColor" strokeWidth="4"/>
  </svg>
);

export const GridShiftBackIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="15.5,18.5 15.5,27.5 8.5,23" fill="currentColor"/> <rect className="dim" x="24.15" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="28.95" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="33.55" y="13.4" width="1.5" height="19.2" fill="currentColor"/>
  </svg>
);

export const GridShiftForwardIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <rect className="dim" x="12.95" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="17.55" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="22.35" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <polygon points="32.5,18.5 32.5,27.5 39.5,23" fill="currentColor"/>
  </svg>
);

export const GridUndoIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="17,16 24.5,11.8 24.5,20.5" fill="currentColor"/> <path d="M25.43 16.52A6.5 6.5 0 1 1 24.32 29.28" fill="none" stroke="currentColor" strokeWidth="4"/>
  </svg>
);

export const GridWidenIcon = (props: IconProps) => (
  <svg viewBox="0 0 48 46" aria-hidden focusable="false" {...props}>
    <polygon points="14.5,18.5 14.5,27.5 6.5,23" fill="currentColor"/> <rect className="dim" x="19.25" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="24.15" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <rect className="dim" x="28.95" y="13.4" width="1.5" height="19.2" fill="currentColor"/> <polygon points="35.5,18.5 35.5,27.5 42.5,23" fill="currentColor"/>
  </svg>
);

export const HeadphonesIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <path d="M2.4 10.4V8.4a5.6 5.6 0 0 1 11.2 0v2.0" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/> <rect x="1.3" y="9.7" width="2.9" height="4.4" rx="1.35" fill="currentColor"/> <rect x="11.8" y="9.7" width="2.9" height="4.4" rx="1.35" fill="currentColor"/>
  </svg>
);

export const HistoryIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <circle cx="8" cy="8" r="5.6" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M8 4.6V8l2.6 1.6" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const InfoIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <circle cx="8" cy="8" r="6.4" fill="currentColor"/> <rect x="7.1" y="6.6" width="1.8" height="5.0" rx="0.6" fill="var(--c-black)"/> <circle cx="8" cy="4.6" r="1.05" fill="var(--c-black)"/>
  </svg>
);

export const LayoutBrowserIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="1.5" y="3.4" width="13" height="9.2" fill="none" stroke="currentColor" strokeWidth="1.4"/><path d="M1.5 6.6h13" stroke="currentColor" strokeWidth="1.4"/>
  </svg>
);

export const LayoutDualIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="1.5" y="5.4" width="8.4" height="7.2" fill="none" stroke="currentColor" strokeWidth="1.4"/><rect x="6.1" y="3.4" width="8.4" height="7.2" fill="none" stroke="currentColor" strokeWidth="1.4"/>
  </svg>
);

export const LayoutOneIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="1.5" y="4" width="13" height="8" fill="none" stroke="currentColor" strokeWidth="1.4"/><rect x="3.8" y="6.4" width="3.2" height="3.2" fill="currentColor"/>
  </svg>
);

export const LayoutSimpleIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="1.5" y="5.6" width="13" height="4.8" fill="none" stroke="currentColor" strokeWidth="1.4"/>
  </svg>
);

export const LayoutTwoIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <rect x="1.5" y="3" width="13" height="4.4" fill="none" stroke="currentColor" strokeWidth="1.4"/><rect x="1.5" y="8.6" width="13" height="4.4" fill="none" stroke="currentColor" strokeWidth="1.4"/>
  </svg>
);

export const LinkOnIcon = (props: IconProps) => (
  <svg viewBox="0 0 28 28" aria-hidden focusable="false" {...props}>
    <circle cx="6.5" cy="21.5" r="3.4" fill="currentColor"/> <path fill="none" stroke="currentColor" strokeWidth="4.2" strokeLinecap="butt" d="M6.5 11.5a10 10 0 0 1 10 10"/> <path fill="none" stroke="currentColor" strokeWidth="4.2" strokeLinecap="butt" d="M6.5 3.5a18 18 0 0 1 18 18"/>
  </svg>
);

export const LinkIcon = (props: IconProps) => (
  <svg viewBox="0 0 57 45" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M0 11.5h35.5V0L57 19H0zM57 33.5H21.5V45L0 26h57z"/>
  </svg>
);

export const ListIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 12" aria-hidden focusable="false" {...props}>
    <rect x="1" y="1" width="12" height="10" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M1 4h12M1 7.5h12M4.5 4v7" stroke="currentColor" strokeWidth="1.4"/>
  </svg>
);

export const LoopInIcon = (props: IconProps) => (
  <svg viewBox="0 0 15 6" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M0 3h3l3-3v3h9v3H0z"/>
  </svg>
);

export const LoopOutIcon = (props: IconProps) => (
  <svg viewBox="0 0 15 6" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M15 3h-3L9 0v3H0v3h15z"/>
  </svg>
);

export const MagnifierMinusIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 14" aria-hidden focusable="false" {...props}>
    <circle cx="5.75" cy="5.75" r="4.75" fill="none" stroke="currentColor" strokeWidth="1.5"/> <path d="M9.4 9.4 13 13" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"/> <path d="M3.4 5.75h4.7" fill="none" stroke="currentColor" strokeWidth="1.3"/>
  </svg>
);

export const MagnifierPlusIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 14" aria-hidden focusable="false" {...props}>
    <circle cx="5.75" cy="5.75" r="4.75" fill="none" stroke="currentColor" strokeWidth="1.5"/> <path d="M9.4 9.4 13 13" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"/> <path d="M5.75 3.4v4.7M3.4 5.75h4.7" fill="none" stroke="currentColor" strokeWidth="1.3"/>
  </svg>
);

export const NoteIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 12" aria-hidden focusable="false" {...props}>
    <path d="M9 0v8.2a2.3 2.3 0 1 1-1.4-2.1V2.2L4 3.2v6.2a2.3 2.3 0 1 1-1.4-2.1V1.6z" fill="currentColor"/>
  </svg>
);

export const PrefAboutIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" fillRule="evenodd" d="M8 .5a7.5 7.5 0 1 1 0 15 7.5 7.5 0 0 1 0-15zm0 1.6a5.9 5.9 0 1 0 0 11.8A5.9 5.9 0 0 0 8 2.1zM8 6.6c.5 0 .9.4.9.9v4a.9.9 0 0 1-1.8 0v-4c0-.5.4-.9.9-.9zm0-2.8a1.1 1.1 0 1 1 0 2.2 1.1 1.1 0 0 1 0-2.2z"/>
  </svg>
);

export const PrefAdvancedIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 12" aria-hidden focusable="false" {...props}>
    <g fill="currentColor"> <rect x="0" y="0.5" width="16" height="1.6"/> <rect x="0" y="5.2" width="16" height="1.6"/> <rect x="0" y="9.9" width="16" height="1.6"/> </g>
  </svg>
);

export const PrefAnalysisIcon = (props: IconProps) => (
  <svg viewBox="0 0 18 14" aria-hidden focusable="false" {...props}>
    <g fill="currentColor"> <rect x="0" y="5" width="1.6" height="4"/> <rect x="2.8" y="3" width="1.6" height="8"/> <rect x="5.6" y="4" width="1.6" height="6"/> <rect x="8.2" y="0" width="1.6" height="14"/> <rect x="10.8" y="4" width="1.6" height="6"/> <rect x="13.6" y="3" width="1.6" height="8"/> <rect x="16.4" y="5" width="1.6" height="4"/> </g>
  </svg>
);

export const PrefAudioIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <path d="M1 5.5h3.4L9 2v12L4.4 10.5H1z" fill="currentColor"/> <rect x="11" y="5.5" width="2.6" height="5" fill="currentColor"/>
  </svg>
);

export const PrefDjSystemIcon = (props: IconProps) => (
  <svg viewBox="0 0 18 14" aria-hidden focusable="false" {...props}>
    <rect x="0.7" y="0.7" width="16.6" height="12.6" rx="1" fill="none" stroke="currentColor" strokeWidth="1.4"/> <circle cx="5.5" cy="7" r="2.8" fill="none" stroke="currentColor" strokeWidth="1.3"/> <circle cx="5.5" cy="7" r="0.9" fill="currentColor"/> <g fill="currentColor"> <rect x="10.5" y="3.5" width="4.5" height="1.3"/> <rect x="10.5" y="6.3" width="4.5" height="1.3"/> <rect x="10.5" y="9.1" width="4.5" height="1.3"/> </g>
  </svg>
);

export const PrefKeyboardIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" fillRule="evenodd" d="M2 0.5h12A1.5 1.5 0 0 1 15.5 2v12a1.5 1.5 0 0 1-1.5 1.5H2A1.5 1.5 0 0 1 .5 14V2A1.5 1.5 0 0 1 2 .5zM7 3.5 3.6 12.8h1.9l.8-2.4h3.4l.8 2.4h1.9L9 3.5zm1 2.4L6.8 8.9h2.4z"/>
  </svg>
);

export const PrefViewIcon = (props: IconProps) => (
  <svg viewBox="0 0 18 13" aria-hidden focusable="false" {...props}>
    <path d="M9 1C5.2 1 2.2 3.6 1 6.5 2.2 9.4 5.2 12 9 12s6.8-2.6 8-5.5C15.8 3.6 12.8 1 9 1z" fill="none" stroke="currentColor" strokeWidth="1.4"/> <circle cx="9" cy="6.5" r="2.6" fill="currentColor"/>
  </svg>
);

export const RecordIcon = (props: IconProps) => (
  <svg viewBox="0 0 100 100" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" fillRule="evenodd" d="M50 7a43 43 0 1 0 0 86 43 43 0 0 0 0-86zm0 31a12 12 0 1 0 0 24 12 12 0 0 0 0-24z"/>
  </svg>
);

export const ReloadIcon = (props: IconProps) => (
  <svg viewBox="0 0 12 15" aria-hidden focusable="false" {...props}>
    <path d="M9.6 4.2A4.6 4.6 0 0 0 2 5.6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/> <path d="M2.4 9.8A4.6 4.6 0 0 0 10 8.4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/> <path d="M8.2 1.2 10.2 4.4 6.6 4.6z" fill="currentColor"/> <path d="M3.8 13.8 1.8 10.6 5.4 10.4z" fill="currentColor"/>
  </svg>
);

export const SmartListIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 12" aria-hidden focusable="false" {...props}>
    <rect x="1" y="1" width="12" height="10" fill="none" stroke="currentColor" strokeWidth="1.4"/> <path d="M1 4h12M4.5 4v7" stroke="currentColor" strokeWidth="1.4"/> <path d="M9 5.6v3.8M7.4 6.5l3.2 2M7.4 8.5l3.2-2" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round"/>
  </svg>
);

export const SortDownIcon = (props: IconProps) => (
  <svg viewBox="0 0 12 12" aria-hidden focusable="false" {...props}>
    <path d="M6 0.6v9.9M2.2 6.7 6 10.5l3.8-3.8" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const SortUpIcon = (props: IconProps) => (
  <svg viewBox="0 0 12 12" aria-hidden focusable="false" {...props}>
    <path d="M6 11.4V1.5M2.2 5.3 6 1.5l3.8 3.8" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const SpinnerIcon = (props: IconProps) => (
  <svg viewBox="0 0 10 16" aria-hidden focusable="false" {...props}>
    <path d="m1.5 5.5 3.5-3.5 3.5 3.5M1.5 10.5 5 14l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const StarEmptyIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 13.31" aria-hidden focusable="false" {...props}>
    <path fillRule="evenodd" fill="currentColor" d="M7 0L8.82 4.86L14 5.09L9.94 8.32L11.33 13.31L7 10.45L2.67 13.31L4.06 8.32L0 5.09L5.18 4.86ZM5.64 5.49L1.77 5.66L4.8 8.07L3.76 11.81L7 9.67L10.24 11.81L9.2 8.07L12.23 5.66L8.36 5.49L7 1.86Z"/>
  </svg>
);

export const StarLitIcon = (props: IconProps) => (
  <svg viewBox="0 0 14 13.31" aria-hidden focusable="false" {...props}>
    <path fill="currentColor" d="M7 0L8.82 4.86L14 5.09L9.94 8.32L11.33 13.31L7 10.45L2.67 13.31L4.06 8.32L0 5.09L5.18 4.86Z"/>
  </svg>
);

export const SubBrowseIcon = (props: IconProps) => (
  <svg viewBox="0 0 18 18" aria-hidden focusable="false" {...props}>
    <rect x="3" y="4" width="6" height="10" fill="currentColor"/> <rect x="3.5" y="4.5" width="11" height="9" fill="none" stroke="currentColor" strokeWidth="1"/>
  </svg>
);

export const SyncIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" aria-hidden focusable="false" {...props}>
    <path d="M3.2 6.6A5.2 5.2 0 0 1 12.4 4.6" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"/> <path d="M12.8 1.6v3.4H9.4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/> <path d="M12.8 9.4A5.2 5.2 0 0 1 3.6 11.4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round"/> <path d="M3.2 14.4V11h3.4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const TickIcon = (props: IconProps) => (
  <svg viewBox="0 0 12 8" aria-hidden focusable="false" {...props}>
    <path d="M1 4.1L4.1 7 11 1" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);

export const TwistyIcon = (props: IconProps) => (
  <svg viewBox="0 0 8 8" aria-hidden focusable="false" {...props}>
    <path d="m2.6 1.4 3 2.6-3 2.6" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);
