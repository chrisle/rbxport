import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { COLOR_NAMES } from "@/lib/trackFilter";

const tokens = readFileSync("src/styles/tokens.css", "utf8");
const table = readFileSync("src/views/browser/TrackTable.module.css", "utf8");

function token(name: string): string | undefined {
  return new RegExp(`--${name}:\\s*([^;]+);`).exec(tokens)?.[1]?.trim();
}

/**
 * rekordbox 7.2.19's track colours, `rekordbox::cPink` .. `cPurple` in the
 * ColorTheme static initialiser, which `BrowseHelper::findBallColour` hands to
 * the track list's Color column. Red and Green were also read off the column
 * on Windows (issue #312, Winrig chris-win11): (248,0,0) and (0,224,0).
 */
const REKORDBOX_BALLS: Record<string, string> = {
  Pink: "#F870F8", Red: "#F80000", Orange: "#F8A030", Yellow: "#F8E330",
  Green: "#00E000", Aqua: "#00C0F8", Blue: "#0050F8", Purple: "#9808F8",
};

describe("the track list's Color column", () => {
  it("paints each track colour in rekordbox's own RGB", () => {
    for (const name of COLOR_NAMES) {
      expect(token(`c-comment-${name.toLowerCase()}`), name).toBe(REKORDBOX_BALLS[name]);
      expect(table, name).toContain(
        `.colorBall[data-color="${name}"] { background: var(--c-comment-${name.toLowerCase()}); }`,
      );
    }
  });

  it("draws an 8px ball at x 5 and the name from x 16, as paintColourColumn does", () => {
    expect(token("s-color-col-ball-x")).toBe("5px");
    expect(token("s-color-col-text-x")).toBe("16px");
    expect(token("s-filter-dot")).toBe("8px");
  });
});
