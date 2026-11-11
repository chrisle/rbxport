/**
 * The INFO tab beside the deck: `Show INFO Panel` in `german.lang`.
 *
 * `[DOC]` The rekordbox 6.0.0 manual, p.57, lists what the panel holds and
 * pictures it: the rating's five stars, a hairline, the colour as a dot and
 * its name, a hairline, the comment behind a speech bubble, a hairline, then
 * four plain lines — file type, file size, sample rate, bit rate. No labels;
 * no title, artist, BPM, key or time, which the deck's own header already
 * shows. Every size here is measured off that graphic and recorded as a
 * token source.
 *
 * `[ASSUME]` That 7.2.11 draws the tab the same way. No capture of tonight's
 * has the INFO tab selected — every one shows MEMORY — so the box it fills
 * is measured from the MEMORY list, the face from the MEMORY rows, and the
 * arrangement inside it from the manual. Read-only: the rating and comment
 * are edited in the browser and the information panel.
 */
import { memo } from "react";

import type { RowDto, TrackDetails } from "@/ipc/types";
import { CommentIcon } from "@/components/icons";
import { deckInfo } from "./deckInfoLines";
import { RatingStar } from "@/components/RatingStar";
import { useColorNames } from "@/store/useColorNames";
import styles from "./DeckInfo.module.css";

export const DeckInfo = memo(function DeckInfo({
  track, details,
}: {
  track: RowDto | null;
  details: TrackDetails | null;
}) {
  const info = deckInfo(track, details, useColorNames());
  return (
    <div className={styles.info} data-testid="deck-info">
      <div className={styles.rating} aria-label={`Rating ${info.rating} of 5`}>
        {[1, 2, 3, 4, 5].map((star) => (
          <span key={star} aria-hidden><RatingStar lit={info.rating >= star} className={styles.starIcon} /></span>
        ))}
      </div>
      <div className={styles.color} data-testid="deck-info-color">
        {info.color ? (
          <>
            <span className={styles.dot} data-color={info.dot} aria-hidden />
            <span className={styles.line}>{info.color}</span>
          </>
        ) : null}
      </div>
      <div className={styles.comment}>
        <CommentIcon className={styles.bubble} />
        <span className={styles.commentText} data-testid="deck-info-comment">{info.comment}</span>
      </div>
      <div className={styles.file} data-testid="deck-info-file">
        {info.file.map((line, at) => (
          // Four fixed lines, so the block does not jump between a track
          // and none; the position is the key because the lines are the
          // same four facts in the same order every time.
          <span key={at} className={styles.line}>{line}</span>
        ))}
      </div>
    </div>
  );
});
