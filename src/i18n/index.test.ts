import { describe, expect, it } from "vitest";

import uiStrings from "./ui.json";
import de from "../../public/locales/de.json";
import ja from "../../public/locales/ja.json";
import { LANGUAGE_CHOICES, translate, type Catalog } from ".";
import { LOCALES } from "@/lib/preferences";

describe("localization", () => {
  it("offers every rekordbox language in its native name", () => {
    expect(LANGUAGE_CHOICES.map(({ value }) => value)).toEqual(LOCALES);
    expect(LANGUAGE_CHOICES.at(-1)).toEqual({ value: "ja", label: "日本語" });
  });

  it("uses the canonical catalogs and falls back to English", () => {
    expect(translate("Collection", de)).toBe("Sammlung");
    expect(translate("Collection", ja)).not.toBe("Collection");
    expect(translate("rbxport-only wording", de)).toBe("rbxport-only wording");
  });

  it("translates backend messages that carry a track or playlist name", () => {
    expect(translate("OneLibrary: 'Intro Mix' changed on the USB. Import its changes before syncing.", de))
      .toBe("OneLibrary: „Intro Mix“ wurde auf dem USB-Stick geändert. Importieren Sie die Änderungen vor dem Synchronisieren.");
    expect(translate("Device-only playlist 'Warmup' would be lost", de))
      .toBe("Die nur auf dem Gerät vorhandene Wiedergabeliste „Warmup“ ginge verloren");
  });

  it("translates runtime messages while preserving their inserted details", () => {
    const catalog = { "Writing to {device}…": "Schreibe auf {device}…" };
    expect(translate("Writing to USB A…", catalog)).toBe("Schreibe auf USB A…");
  });

  it("has a translation entry for every detected UI string in every locale", async () => {
    for (const locale of LOCALES.filter((value) => value !== "en")) {
      const catalog = (await import(`../../public/locales/${locale}.json`)).default as Catalog;
      expect(uiStrings.filter((text) => catalog[text] === undefined), locale).toEqual([]);
    }
  });

  it("never shows a blank string for a non-blank one", async () => {
    // translate() shows "" verbatim; rekordbox's own .lang files leave some
    // sentences blank (the device import copy question, #248), so a copied
    // blank would leave a dialog without its explanation or labels.
    for (const locale of LOCALES.filter((value) => value !== "en")) {
      const catalog = (await import(`../../public/locales/${locale}.json`)).default as Catalog;
      expect(Object.keys(catalog).filter((text) => text !== "" && catalog[text] === ""), locale).toEqual([]);
    }
  });

  it("preserves named placeholders in translated dynamic messages", async () => {
    const placeholders = (text: string) => text.match(/\{[a-z][a-z0-9]*\}/gi) ?? [];
    const dynamic = uiStrings.filter((text) => placeholders(text).length > 0);
    for (const locale of LOCALES.filter((value) => value !== "en")) {
      const catalog = (await import(`../../public/locales/${locale}.json`)).default as Catalog;
      for (const source of dynamic) expect(placeholders(catalog[source] ?? ""), `${locale}: ${source}`).toEqual(placeholders(source));
    }
  });
});
