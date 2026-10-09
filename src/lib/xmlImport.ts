import type { Backend } from "@/ipc/types";

/**
 * rekordbox's question before an xml or iTunes playlist import replaces
 * lists that already stand under the same name, asked OK/Cancel under the
 * title "Import". [OBS static, rekordbox 7.2.19 arm64:
 * `browse::TreeViewer::treeMessageImportPlaylistFromBridge` @0x101569848-
 * 0x1015698c8 joins the two translated sentences with "\n" and calls
 * `BrowseAlertWindow::showOkCancelBox`; Cancel imports nothing.]
 */
export function askToReplaceLists(
  backend: Pick<Backend, "confirm">,
  t: (text: string) => string,
): Promise<boolean> {
  const message = `${t("One or several lists with the same name already exist.")}\n${t("Do you want to replace them with the one you're importing?")}`;
  return backend.confirm(message, { yes: t("OK"), no: t("Cancel"), title: t("Import") });
}
