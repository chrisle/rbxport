/** The browser header's containing-playlist paths for one collection row. */
import { useTranslation } from "@/i18n";
import { useTrackPlaylists } from "@/store/useTrackPlaylists";
import styles from "./TrackTable.module.css";

export function PlaylistPath({ caption, track, owner, onChoose }: {
  caption: string;
  track: string | null;
  owner: string;
  onChoose: (playlist: string | null, track: string) => void;
}) {
  const t = useTranslation();
  const paths = useTrackPlaylists(track, owner);
  return <select className={styles.playlistPath} aria-label="Playlists" value=""
    disabled={paths === null} onChange={event => {
      const playlist = event.currentTarget.value;
      if (track !== null && paths !== null) {
        if (playlist === "@collection") onChoose(null, track);
        else if (paths.some(path => path.id === playlist)) onChoose(playlist, track);
      }
    }}>
    <option value="" data-i18n-ignore>{caption}</option>
    {paths !== null ? <option value="@collection">{t("Collection")}</option> : null}
    {paths?.map(path => <option key={path.id} value={path.id} data-i18n-ignore>
      {[t("Playlists"), path.path, path.name].filter(Boolean).join(" / ")}
    </option>)}
  </select>;
}
