import { getBackend } from "@/ipc/client";
import type { Backend, XmlImportReport } from "@/ipc/types";

type Translate = (text: string, values?: Readonly<Record<string, string | number>>) => string;
type Note = { text: string; failed: boolean; busy?: boolean } | null;

/**
 * rekordbox's question before an xml or iTunes playlist import replaces
 * lists that already stand under the same name, asked OK/Cancel under the
 * title "Import". [OBS static, rekordbox 7.2.19 arm64:
 * `browse::TreeViewer::treeMessageImportPlaylistFromBridge` @0x101569848-
 * 0x1015698c8 joins the two translated sentences with "\n" and calls
 * `BrowseAlertWindow::showOkCancelBox`; Cancel imports nothing.]
 */
export function askToReplaceLists(backend: Pick<Backend, "confirm">, t: Translate): Promise<boolean> {
  const message = `${t("One or several lists with the same name already exist.")}\n${t("Do you want to replace them with the one you're importing?")}`;
  return backend.confirm(message, { yes: t("OK"), no: t("Cancel"), title: t("Import") });
}

/** The status line after File › Import rekordbox xml or iTunes Library. */
export function importSummary(imported: XmlImportReport): string {
  const parts = [
    `${imported.imported} track${imported.imported === 1 ? "" : "s"} imported`,
    imported.existing > 0 ? `${imported.existing} already here` : "",
    imported.skipped.length > 0 ? `${imported.skipped.length} skipped` : "",
    `${imported.playlists} playlist${imported.playlists === 1 ? "" : "s"}`,
    imported.cues > 0 ? `${imported.cues} cue${imported.cues === 1 ? "" : "s"}` : "",
  ].filter((part) => part !== "");
  return `${parts.join(", ")}.`;
}

/**
 * File › Import rekordbox xml or Import iTunes Library: picks the file,
 * imports it with its progress on the status line, and hands what landed to
 * `landed`. Before replacing lists already in the library it asks
 * rekordbox's question (#152); Cancel imports nothing.
 */
export async function importCollection(
  source: "rekordbox" | "itunes",
  t: Translate,
  setNote: (note: Note) => void,
  landed: (backend: Backend, imported: XmlImportReport) => Promise<void>,
): Promise<void> {
  setNote({ text: source === "itunes" ? "Choosing the iTunes Library.xml…" : "Choosing a rekordbox XML file…", failed: false });
  let stopProgress = () => {};
  let finished = false;
  try {
    const backend = await getBackend();
    stopProgress = backend.onImportProgress((p) => {
      if (finished) return;
      setNote({
        text: p.total > 0
          ? t("Importing {done} of {total} tracks…", { done: p.done.toLocaleString(), total: p.total.toLocaleString() })
          : t("Importing…"),
        failed: false,
        busy: true,
      });
    });
    const confirmReplace = () => askToReplaceLists(backend, t);
    const imported = source === "itunes"
      ? await backend.importItunes(confirmReplace)
      : await backend.importXml(confirmReplace);
    if (imported === null) {
      setNote(null);
      return;
    }
    setNote({ text: importSummary(imported), failed: false });
    await landed(backend, imported);
  } catch (e) {
    setNote({ text: e instanceof Error ? e.message : "That XML could not be imported.", failed: true });
  } finally {
    finished = true;
    stopProgress();
  }
}
