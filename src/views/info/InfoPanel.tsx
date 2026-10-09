/**
 * The track information panel.
 *
 * rekordbox calls it the Information Window and keeps it on the right of the
 * browser. Its width is the one in the user's own `browseSetting.xml` —
 * `ListInfo w=557` — and it starts closed, which is what that file records.
 *
 * Three tabs, drawn from the captures `docs/screenshots/… 9.09.33`, `.36`
 * and `.45 PM`:
 *
 * - **Summary**: the sleeve with Track Title / Artist / Album beside it, then
 *   a two-column table of Time, File Type, Size, Date Created, Sample Rate,
 *   Bitrate, DJ Play Count and Location.
 * - **Info**: the editable form, laid out as captured. What the writer will
 *   take is live; what it will not — the album artist (it lives on the
 *   shared album row), BPM (it also lives in the analysis grid), Mix Name,
 *   Message and the two flags (read from columns whose spelling on write has
 *   not been seen), and the Release Date box (its three segments' order is
 *   assumed) — is drawn read-only with the reason in its tooltip.
 * - **Artwork**: the picture large and centred, with an import and a delete
 *   button beneath. Import files the image where rekordbox files an imported
 *   sleeve (`/PIONEER/Artwork/<3 hex>/<uuid>/artwork.jpg` [OBS]; the three
 *   hex digits taken as the uuid's own first three [ASSUME]) and points the
 *   track at it; delete points it at nothing and leaves the file.
 *
 * The reload glyph right of the tabs is drawn and inert: it matches the
 * `brws_refresh` shape, which suggests Reload Tag, but nothing confirms it.
 *
 * With several tracks selected the panel shows and edits them as one, as
 * rekordbox does [OBS: rekordbox 7 on Windows 11; static: 7.2.11
 * `browse::TrackInfoConcreteMediator`]: Summary is greyed and the panel
 * moves to Info, which shows each value the tracks share and leaves the rest
 * blank (see `selectionView`); the Track Title box is greyed; every edit is
 * written to every selected track; and Artwork shows the picture only when
 * the tracks share it, its buttons acting on all of them.
 *
 * Every geometry and colour here is a token measured from those captures.
 */
import { memo, useCallback, useEffect, useRef, useState } from "react";

import { Artwork } from "@/components/Artwork";
import {
  ArtworkDeleteIcon, ArtworkImportIcon, ClearCircleIcon, RecordIcon, ReloadIcon, SpinnerIcon,
} from "@/components/icons";
import { getBackend } from "@/ipc/client";
import type { Backend, RowDto, SelectionDetails, TrackDetails, TrackField, TrackLookups } from "@/ipc/types";
import { RatingStar } from "@/components/RatingStar";
import { useTranslation } from "@/i18n";
import styles from "./InfoPanel.module.css";
import {
  acceptable, COLORS, dateSegments, selectionView, singleView, summaryFacts, type InfoView,
} from "./fields";
import { useTooltip } from "@/store/usePreferences";

export type InfoTab = "summary" | "info" | "artwork";

export interface InfoPanelProps {
  /** The one track shown, when the selection is not several tracks. */
  track: RowDto | null;
  /**
   * The browser's selected track ids, in the order its list reports them.
   * More than one is a multiple selection, shown and edited as a whole.
   */
  selection: readonly string[];
  /** The library is open read-only, so the form is greyed and says why. */
  readOnly: boolean;
  /** Bumped after every edit; the record is read again when it changes. */
  libraryGeneration: number;
  onRate: (ids: readonly string[], stars: number) => void;
  onComment: (ids: readonly string[], comment: string) => void;
  /** Runs one write and reports its outcome, refusals included. */
  onEdit: (what: string, edit: (b: Backend) => Promise<unknown>) => Promise<void>;
}

/** Why a read-only field is read-only, shown in its tooltip. */
const LOCKED: Record<string, string> = {
  albumArtist:
    "The album artist is stored on the album, which every track of the album shares; " +
    "whether rekordbox edits that row or makes a new album is not known.",
  bpm: "The BPM also lives in the analysis grid; editing one without the other is not known to be safe.",
  releaseDate: "The order of the three date boxes is assumed from their widths; the date is shown, not edited.",
  mixName: "Read from the Subtitle column, which holds mix names in the reference library; not written until a capture confirms it.",
  message: "Read from the DeliveryComment column, which is never filled in the reference library; not written.",
  hotCueAutoLoad: "Every track in the reference library carries \"on\"; what an unticked box is stored as has not been seen.",
  publish: "Read from DeliveryControl; how rekordbox spells an unticked box (empty or NULL both occur) has not been seen.",
};

const READ_ONLY_REASON = "The library is read-only. Check Library Protection in Preferences, and quit rekordbox to edit";

export function InfoPanel({
  track, selection, readOnly, libraryGeneration, onRate, onComment, onEdit,
}: InfoPanelProps) {
  const [tab, setTab] = useState<InfoTab>("summary");
  const [details, setDetails] = useState<TrackDetails | null>(null);
  const [lookups, setLookups] = useState<TrackLookups | null>(null);

  const multiple = selection.length > 1;
  // rekordbox greys Summary for several tracks and moves to Info, and stays
  // on Info when the selection is one track again [OBS].
  if (multiple && tab === "summary") setTab("info");
  const trackId = multiple ? null : (track?.id ?? null);

  // The selection's record, read again when the selection or the library
  // changes; one that lands after the selection has moved on is dropped.
  const [several, setSeveral] = useState<{ ids: readonly string[]; record: SelectionDetails } | null>(null);
  useEffect(() => {
    if (!multiple) {
      setSeveral(null);
      return;
    }
    let live = true;
    void getBackend()
      .then((b) => b.selectionDetails(selection))
      .then((record) => {
        if (live) setSeveral({ ids: selection, record });
      })
      .catch(() => {
        if (live) setSeveral(null);
      });
    return () => {
      live = false;
    };
  }, [multiple, selection, libraryGeneration]);
  const selectionRecord = several && several.ids === selection ? several.record : null;
  // A new selection is a new form: drafts typed for the last one go.
  const [selectionKey, setSelectionKey] = useState(0);
  const [keyedSelection, setKeyedSelection] = useState(selection);
  if (keyedSelection !== selection) {
    setKeyedSelection(selection);
    setSelectionKey((k) => k + 1);
  }

  // The record is fetched per track and again after every edit; a fetch
  // that lands after the selection has moved on is dropped.
  useEffect(() => {
    if (trackId === null) {
      setDetails(null);
      return;
    }
    let live = true;
    void getBackend()
      .then((b) => b.trackDetails(trackId))
      .then((d) => {
        if (live && d.id === trackId) setDetails(d);
      })
      .catch(() => {
        // The row's own fields stand in; the rest of the panel stays blank.
        if (live) setDetails(null);
      });
    return () => {
      live = false;
    };
  }, [trackId, libraryGeneration]);

  // The dropdown lists are wanted once the Info tab is shown, and again
  // after an edit, since a new genre is a new option.
  useEffect(() => {
    if (tab !== "info") return;
    let live = true;
    void getBackend()
      .then((b) => b.trackLookups())
      .then((l) => {
        if (live) setLookups(l);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [tab, libraryGeneration]);

  const record = details && details.id === trackId ? details : null;

  const tip = useTooltip();
  return (
    <aside className={styles.panel} aria-label="Information">
      <div className={styles.tabs} role="tablist" aria-label="Information">
        {(["summary", "info", "artwork"] as const).map((id) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`info-tab-${id}`}
            aria-selected={tab === id}
            aria-controls={`info-panel-${id}`}
            className={styles.tab}
            disabled={multiple && id === "summary"}
            onClick={() => setTab(id)}
          >
            {TAB_LABEL[id]}
          </button>
        ))}
        <button
          type="button"
          className={styles.reload}
          aria-label="Reload Tag"
          title={tip("Not wired: the glyph matches rekordbox's refresh, but what it reloads is unconfirmed.")}
          disabled
        >
          <ReloadIcon className={styles.reloadGlyph} />
        </button>
      </div>

      {multiple ? (
        tab === "artwork" ? (
          <div role="tabpanel" id="info-panel-artwork" aria-labelledby="info-tab-artwork" className={styles.artwork}>
            <ArtworkTab
              ids={selection}
              shown={selectionRecord && !selectionRecord.mixed.includes("artwork") && selectionRecord.first.hasArtwork
                ? selectionRecord.first.id
                : null}
              hue={0}
              removable={selectionRecord !== null &&
                (selectionRecord.first.hasArtwork || selectionRecord.mixed.includes("artwork"))}
              readOnly={readOnly}
              onEdit={onEdit}
            />
          </div>
        ) : (
          <div role="tabpanel" id="info-panel-info" aria-labelledby="info-tab-info" className={styles.info}>
            {readOnly ? (
              <p className={styles.banner} role="status">{READ_ONLY_REASON}</p>
            ) : null}
            <InfoForm
              key={`selection-${selectionKey}`}
              view={selectionView(selection, selectionRecord)}
              lookups={lookups}
              readOnly={readOnly}
              onRate={onRate}
              onComment={onComment}
              onEdit={onEdit}
            />
          </div>
        )
      ) : track === null ? (
        <p className={styles.empty}>Select a track.</p>
      ) : tab === "summary" ? (
        <div role="tabpanel" id="info-panel-summary" aria-labelledby="info-tab-summary" className={styles.summary}>
          <Summary track={track} details={record} />
        </div>
      ) : tab === "info" ? (
        <div role="tabpanel" id="info-panel-info" aria-labelledby="info-tab-info" className={styles.info}>
          {readOnly ? (
            <p className={styles.banner} role="status">{READ_ONLY_REASON}</p>
          ) : null}
          <InfoForm
            key={track.id}
            view={singleView(track, record)}
            lookups={lookups}
            readOnly={readOnly}
            onRate={onRate}
            onComment={onComment}
            onEdit={onEdit}
          />
        </div>
      ) : (
        <div role="tabpanel" id="info-panel-artwork" aria-labelledby="info-tab-artwork" className={styles.artwork}>
          <ArtworkTab
            ids={[track.id]}
            shown={(record?.hasArtwork ?? track.hasArtwork) ? track.id : null}
            hue={track.artworkHue}
            removable={record?.hasArtwork ?? track.hasArtwork}
            readOnly={readOnly}
            onEdit={onEdit}
          />
        </div>
      )}
    </aside>
  );
}

const TAB_LABEL: Record<InfoTab, string> = { summary: "Summary", info: "Info", artwork: "Artwork" };

// ------------------------------------------------------------------ Summary

const Summary = memo(function Summary({
  track, details,
}: {
  track: RowDto;
  details: TrackDetails | null;
}) {
  const title = details?.title ?? track.title;
  const artist = details?.artist ?? track.artist;
  const album = details?.album ?? track.album;
  const tip = useTooltip();
  return (
    <>
      <div className={styles.summaryHead}>
        <div className={styles.sleeve} style={{ ["--hue" as string]: `${track.artworkHue}deg` }}>
          {track.hasArtwork ? (
            <Artwork trackId={track.id} className={styles.sleeveImage} />
          ) : (
            <RecordIcon className={styles.disc} aria-hidden />
          )}
        </div>
        <dl className={styles.headFields}>
          <div className={styles.headField}>
            <dt className={styles.label}>Track Title</dt>
            <dd className={styles.headTitle}>{title}</dd>
          </div>
          <div className={styles.headField}>
            <dt className={styles.label}>Artist</dt>
            <dd className={styles.headValue}>{artist}</dd>
          </div>
          <div className={styles.headField}>
            <dt className={styles.label}>Album</dt>
            <dd className={styles.headValue}>{album}</dd>
          </div>
        </dl>
      </div>
      <dl className={styles.facts}>
        {summaryFacts(track, details).map((fact) => (
          <div key={fact.label} className={styles.fact}>
            <dt className={styles.label}>{fact.label}</dt>
            <dd className={styles.value} title={tip(fact.value)}>{fact.value}</dd>
          </div>
        ))}
      </dl>
    </>
  );
});

// --------------------------------------------------------------------- Info

interface InfoFormProps {
  view: InfoView;
  lookups: TrackLookups | null;
  readOnly: boolean;
  onRate: (ids: readonly string[], stars: number) => void;
  onComment: (ids: readonly string[], comment: string) => void;
  onEdit: (what: string, edit: (b: Backend) => Promise<unknown>) => Promise<void>;
}

/** The Info tab's labels, from `german.lang`. */
/** What each field is called, for the line the status bar says after a save. */
export const FIELD_LABEL: Record<TrackField, string> = {
  title: "Track Title",
  artist: "Artist",
  album: "Album",
  year: "Year",
  trackNumber: "Track number",
  discNumber: "Disc number",
  originalArtist: "Original Artist",
  composer: "Composer",
  remixer: "Remixer",
  lyricist: "Lyricist",
  playCount: "DJ Play Count",
  genre: "Genre",
  label: "Label",
  key: "Key",
  bpm: "BPM",
};

function InfoForm({ view, lookups, readOnly, onRate, onComment, onEdit }: InfoFormProps) {
  const { ids, text } = view;

  const commit = useCallback(
    (field: TrackField, value: string) => {
      void onEdit(`${FIELD_LABEL[field]} saved.`, (b) => b.edits.setTrackField(ids, field, value));
    },
    [ids, onEdit],
  );

  const setColor = useCallback(
    (value: string) => {
      // "0" rather than NULL for none: 38,671 of the reference library's
      // 38,681 tracks carry "0", and two carry NULL.
      void onEdit(value === "0" ? "Color cleared." : "Color saved.", (b) =>
        b.edits.setTrackColor(ids, value),
      );
    },
    [ids, onEdit],
  );

  const field = (name: TrackField, extra?: Partial<FieldProps>) => (
    <Field
      key={name}
      label={FIELD_LABEL[name]}
      name={name}
      initial={text(name)}
      disabled={readOnly}
      onCommit={(value) => commit(name, value)}
      {...extra}
    />
  );

  const keys = lookups?.keys ?? [];
  const keyValue = text("key");
  const keyOptions = keyValue && !keys.includes(keyValue) ? [keyValue, ...keys] : keys;
  const myTags = view.myTags;

  return (
    <div className={styles.form} data-locked={readOnly ? "" : undefined}>
      {/* rekordbox greys the Track Title box for several tracks
          (`isTrackEditabled`, item 0, refuses more than one) [OBS]. */}
      <div className={styles.rowFull}>
        {field("title", view.multiple ? { disabled: true, greyed: true } : undefined)}
      </div>
      <div className={styles.rowArtist}>
        {field("artist")}
        {field("year", { numeric: true })}
      </div>
      <div className={styles.rowHalves}>
        {field("album")}
        <Locked label="Release Date" name="releaseDate">
          <DateBox iso={view.releaseDate} />
        </Locked>
      </div>
      <div className={styles.rowThirds}>
        <Locked label="Album Artist" name="albumArtist" value={view.albumArtist} />
        {field("trackNumber", { numeric: true })}
        <Locked label="BPM" name="bpm" value={view.bpm} />
      </div>
      <div className={styles.rowThirds}>
        {field("originalArtist")}
        {field("discNumber", { numeric: true })}
        <Select
          label="Key"
          name="key"
          value={keyValue}
          options={keyOptions}
          disabled={readOnly}
          onChange={(value) => commit("key", value)}
        />
      </div>
      <div className={styles.rowHalves}>
        {field("composer")}
        {field("lyricist")}
      </div>
      <div className={styles.rowComments}>
        <CommentBox
          initial={view.comment}
          disabled={readOnly}
          onCommit={(value) => onComment(ids, value)}
        />
        <div className={styles.side}>
          {field("playCount", { numeric: true })}
          <div className={styles.fieldBlock}>
            <span className={styles.formLabel}>Rating</span>
            <Stars rating={view.rating} disabled={readOnly} onRate={(stars) => onRate(ids, stars)} />
          </div>
          <Check label="Allow to auto load HotCue on CDJ/XDJ" name="hotCueAutoLoad" checked={view.hotCueAutoLoad} />
          <Check label="Publish track information" name="publish" checked={view.publish} />
        </div>
      </div>
      <div className={styles.rowFull}>
        <Locked label="Message" name="message" value={view.message} />
      </div>
      <div className={styles.rowRemixer}>
        {field("remixer")}
        <Locked label="Mix Name" name="mixName" value={view.mixName} />
      </div>
      <div className={styles.rowThirds}>
        {field("label")}
        {field("genre", { list: lookups?.genres ?? [] })}
        <Select
          label="Color"
          name="color"
          value={view.color === "" ? "0" : view.color}
          options={["0", ...COLORS.map((c) => c.id)]}
          optionLabel={(v) => COLORS.find((c) => c.id === v)?.name ?? ""}
          disabled={readOnly}
          onChange={setColor}
        />
      </div>
      {/* My Tag: the library's categories, each tag a toggle. Not in the
          captures — rekordbox keeps My Tag in a panel of its own — so this
          is the form's own row, in its own type. One track at a time: it is
          not part of the Information Window rekordbox edits several tracks
          in, so a multiple selection leaves it out. */}
      {lookups && lookups.myTagCategories.length > 0 && !view.multiple ? (
        <div className={styles.rowFull}>
          <div className={styles.fieldBlock}>
            <span className={styles.formLabel}>My Tag</span>
            <div className={styles.tagRows} role="group" aria-label="My Tag">
              {lookups.myTagCategories.map((category) => (
                <div key={category.name} className={styles.tagRow}>
                  <span className={styles.tagCategory}>{category.name}</span>
                  {category.tags.map((tag) => {
                    const on = myTags?.includes(tag.id) ?? false;
                    return (
                      <button
                        key={tag.id}
                        type="button"
                        className={styles.tag}
                        data-on={on || undefined}
                        aria-pressed={on}
                        disabled={readOnly || !myTags}
                        onClick={() => {
                          const current = myTags ?? [];
                          const next = on ? current.filter((t) => t !== tag.id) : [...current, tag.id];
                          const [id] = ids;
                          if (id === undefined) return;
                          void onEdit(`My Tag ${on ? "removed" : "added"}.`, (b) => b.edits.setMyTags(id, next));
                        }}
                      >
                        {tag.name}
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

interface FieldProps {
  label: string;
  name: TrackField;
  initial: string;
  disabled: boolean;
  /** Drawn greyed, as rekordbox draws a box it will not take an edit in. */
  greyed?: boolean;
  numeric?: boolean;
  /** Suggestions, for a box that is also a dropdown. */
  list?: string[];
  onCommit: (value: string) => void;
}

/**
 * One text box. Commits on Enter and on blur — clicking away is how people
 * leave a field — and Escape puts the record's value back, which is the
 * escape hatch that makes committing on blur safe. A value the writer would
 * refuse is put back too, so a typo in the year box never reaches it.
 */
function Field({ label, name, initial, disabled, greyed = false, numeric = false, list, onCommit }: FieldProps) {
  const [draft, setDraft] = useState(initial);
  // A new record for the same track (after an edit) refreshes the box,
  // unless the box is what is being typed in.
  const [seen, setSeen] = useState(initial);
  if (seen !== initial) {
    setSeen(initial);
    setDraft(initial);
  }
  // Escape blurs the box, and the blur would otherwise commit the draft it
  // was meant to throw away: the blur handler closes over the old draft.
  const abandoned = useRef(false);
  const finish = () => {
    if (abandoned.current) {
      abandoned.current = false;
      return;
    }
    if (draft === initial) return;
    if (!acceptable(name, draft)) {
      setDraft(initial);
      return;
    }
    onCommit(draft);
  };
  const inputId = `info-field-${name}`;
  const listId = list ? `${inputId}-list` : undefined;
  return (
    <div className={styles.fieldBlock} data-greyed={greyed ? "" : undefined}>
      <label className={styles.formLabel} htmlFor={inputId}>{label}</label>
      <div className={styles.box} data-list={list ? "" : undefined}>
        <input
          id={inputId}
          className={styles.input}
          value={draft}
          disabled={disabled}
          inputMode={numeric ? "numeric" : undefined}
          list={listId}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={finish}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              finish();
              e.currentTarget.blur();
            } else if (e.key === "Escape") {
              abandoned.current = true;
              setDraft(initial);
              e.currentTarget.blur();
            }
          }}
        />
        {list ? (
          <>
            <SpinnerIcon className={styles.spinner} aria-hidden />
            <datalist id={listId}>
              {list.map((option) => <option key={option} value={option} />)}
            </datalist>
          </>
        ) : null}
      </div>
    </div>
  );
}

/**
 * A box the writer will not take yet. It shows the value and says why in
 * its tooltip. With children, it is the label over a box drawn by the
 * caller — the Release Date's three segments.
 */
function Locked({
  label, name, value, children,
}: {
  label: string;
  name: string;
  value?: string;
  children?: React.ReactNode;
}) {
  const inputId = `info-field-${name}`;
  const tip = useTooltip();
  return (
    <div className={styles.fieldBlock} data-locked="">
      <label className={styles.formLabel} htmlFor={inputId}>{label}</label>
      {children ?? (
        <div className={styles.box}>
          <input
            id={inputId}
            className={styles.input}
            value={value ?? ""}
            readOnly
            title={tip(LOCKED[name])}
            aria-description={LOCKED[name]}
          />
        </div>
      )}
    </div>
  );
}

/** The Release Date's three boxes and its clear button, all inert. */
function DateBox({ iso }: { iso: string }) {
  const [day, month, year] = dateSegments(iso);
  const tip = useTooltip();
  return (
    <div className={styles.dateRow} title={tip(LOCKED["releaseDate"])}>
      <div className={styles.dateBoxes} role="group" aria-label="Release Date" aria-description={LOCKED["releaseDate"]}>
        {[["day", day], ["month", month], ["year", year]].map(([name, part]) => (
          <span key={name} className={styles.dateBox}>
            <span className={styles.dateText}>{part}</span>
            <SpinnerIcon className={styles.spinner} aria-hidden />
          </span>
        ))}
      </div>
      <button type="button" className={styles.dateClear} aria-label="Clear" disabled>
        <ClearCircleIcon className={styles.dateClearGlyph} />
      </button>
    </div>
  );
}

function Select({
  label, name, value, options, optionLabel, disabled, onChange,
}: {
  label: string;
  name: string;
  value: string;
  options: string[];
  optionLabel?: (value: string) => string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const inputId = `info-field-${name}`;
  return (
    <div className={styles.fieldBlock}>
      <label className={styles.formLabel} htmlFor={inputId}>{label}</label>
      <div className={styles.box} data-list="">
        <select
          id={inputId}
          className={styles.select}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
        >
          {!options.includes("") && !options.includes("0") ? <option value="" /> : null}
          {options.map((option) => (
            <option key={option} value={option}>{optionLabel ? optionLabel(option) : option}</option>
          ))}
        </select>
        <SpinnerIcon className={styles.spinner} aria-hidden />
      </div>
    </div>
  );
}

function CommentBox({
  initial, disabled, onCommit,
}: {
  initial: string;
  disabled: boolean;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [seen, setSeen] = useState(initial);
  if (seen !== initial) {
    setSeen(initial);
    setDraft(initial);
  }
  const abandoned = useRef(false);
  return (
    <div className={styles.fieldBlock}>
      <label className={styles.formLabel} htmlFor="info-field-comment">Comments</label>
      <textarea
        id="info-field-comment"
        className={styles.comments}
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          if (abandoned.current) {
            abandoned.current = false;
            return;
          }
          if (draft !== initial) onCommit(draft);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            abandoned.current = true;
            setDraft(initial);
            e.currentTarget.blur();
          }
        }}
      />
    </div>
  );
}

function Stars({
  rating, disabled, onRate,
}: {
  rating: number;
  disabled: boolean;
  onRate: (stars: number) => void;
}) {
  return (
    <span className={styles.stars} role="radiogroup" aria-label="Rating">
      {[1, 2, 3, 4, 5].map((star) => (
        <button
          key={star}
          type="button"
          className={styles.star}
          role="radio"
          aria-checked={rating === star}
          aria-label={`${star} of 5`}
          disabled={disabled}
          // Clicking the star already set clears the rating, which is how
          // rekordbox behaves and the only way to get back to none.
          onClick={() => onRate(rating === star ? 0 : star)}
        >
          <RatingStar lit={star <= rating} className={styles.starIcon} />
        </button>
      ))}
    </span>
  );
}

/** A checkbox that is drawn and read; see `LOCKED` for why it is not written. */
function Check({ label, name, checked }: { label: string; name: string; checked: boolean }) {
  const inputId = `info-field-${name}`;
  const tip = useTooltip();
  return (
    <div className={styles.checkRow} data-locked="" title={tip(LOCKED[name])}>
      <label className={styles.checkLabel} htmlFor={inputId}>{label}</label>
      <input
        id={inputId}
        type="checkbox"
        className={styles.check}
        checked={checked}
        readOnly
        aria-description={LOCKED[name]}
        // A read-only checkbox still toggles on click; a disabled one greys
        // its label, which the capture does not. Swallow the click instead.
        onClick={(e) => e.preventDefault()}
      />
    </div>
  );
}

// ------------------------------------------------------------------ Artwork

/**
 * The Artwork tab, for one track or several. `shown` is the track whose
 * picture is drawn — several tracks draw one only when they all share it,
 * as rekordbox's `tracksHaveSameArtwork` decides — and an import or a
 * delete goes to every track in `ids`, as rekordbox's `addArtwork` files the
 * image for each selected track in turn.
 */
function ArtworkTab({ ids, shown, hue, removable, readOnly, onEdit }: {
  ids: readonly string[];
  shown: string | null;
  hue: number;
  removable: boolean;
  readOnly: boolean;
  onEdit: (what: string, edit: (b: Backend) => Promise<unknown>) => Promise<void>;
}) {
  const tip = useTooltip();
  const t = useTranslation();
  const add = () => {
    void (async () => {
      const backend = await getBackend();
      const image = await backend.pickImage(t("Select an artwork"));
      if (image === null) return;
      await onEdit("Artwork added.", (b) => b.edits.addArtwork(ids, image));
    })();
  };
  const remove = () => {
    void (async () => {
      const backend = await getBackend();
      const question = ids.length > 1
        ? t("Remove the artwork of the {count} selected tracks? The image files stay where they are.", { count: ids.length })
        : t("Remove this track's artwork? The image file stays where it is.");
      if (!(await backend.confirm(question))) return;
      await onEdit("Artwork removed.", (b) => b.edits.clearArtwork(ids));
    })();
  };
  return (
    <div className={styles.artworkArea}>
      <div className={styles.picture} style={{ ["--hue" as string]: `${hue}deg` }}>
        {shown !== null ? (
          <Artwork trackId={shown} className={styles.pictureImage} />
        ) : (
          // What rekordbox draws here without artwork is not captured; the
          // Summary tab's record stands in.
          <RecordIcon className={styles.disc} aria-hidden />
        )}
      </div>
      <div className={styles.artworkButtons}>
        <button
          type="button"
          className={styles.artworkButton}
          aria-label="Add Artwork"
          title={tip(readOnly ? "The library is read-only." : "Add Artwork")}
          disabled={readOnly}
          onClick={add}
        >
          <ArtworkImportIcon className={styles.artworkGlyph} />
        </button>
        <button
          type="button"
          className={styles.artworkButton}
          aria-label="Delete Artwork"
          title={tip(readOnly ? "The library is read-only." : "Delete Artwork")}
          disabled={readOnly || !removable}
          onClick={remove}
        >
          <ArtworkDeleteIcon className={styles.artworkGlyph} />
        </button>
      </div>
    </div>
  );
}
