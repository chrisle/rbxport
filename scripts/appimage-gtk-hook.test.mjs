import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PLUGIN, hookHead, hookMatches, toolsDir } from "./appimage-gtk-hook.mjs";

const plugin = readFileSync(PLUGIN, "utf8");

// What the AppImages through 1.8.2 run (extracted from
// rbxport-1.8.1-linux-x86_64.AppImage): only GNOME's theme name decides.
const SHIPPED_THEME_LINE =
  'gsettings get org.gnome.desktop.interface gtk-theme 2> /dev/null | grep -qi "dark" && GTK_THEME_VARIANT="dark" || GTK_THEME_VARIANT="light"';

/**
 * Runs the plugin's AppRun hook the way AppRun sources it, with `dbus-send`
 * and `gsettings` stubbed: `portal` is the settings portal's color-scheme
 * (null when there is no portal), `keys` the gsettings values by
 * "schema key" (absent keys fail as an uninstalled schema does).
 */
function gtkTheme({ portal = null, keys = {}, env = {} }) {
  const dir = mkdtempSync(join(tmpdir(), "rbxport-448-test-"));
  try {
    const stub = (name, body) => {
      writeFileSync(join(dir, name), `#!/usr/bin/env bash\n${body}\n`);
      chmodSync(join(dir, name), 0o755);
    };
    // dbus-send --print-reply prints the Read reply's nested variant last.
    stub(
      "dbus-send",
      portal === null
        ? "exit 1"
        : `echo "method return time=1 sender=:1.2 -> destination=:1.3 serial=4 reply_serial=2"\necho "   variant       variant          uint32 ${portal}"`,
    );
    const cases = Object.entries(keys)
      .map(([key, value]) => `  "get ${key}") echo "${value}";;`)
      .join("\n");
    stub("gsettings", `case "$*" in\n${cases}\n  *) echo "No such schema" >&2; exit 1;;\nesac`);
    const hook = join(dir, "hook.sh");
    writeFileSync(hook, hookHead(plugin));
    return execFileSync("bash", ["-c", `. "$1"; printf %s "$GTK_THEME"`, "hook", hook], {
      encoding: "utf8",
      env: { PATH: `${dir}:${process.env.PATH}`, HOME: dir, APPDIR: dir, ...env },
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("the hook no longer decides by GNOME's theme name alone", () => {
  assert.ok(!hookHead(plugin).includes(SHIPPED_THEME_LINE));
});

test("Cinnamon's dark mode, through the xapp settings portal, gives the dark menu bar", () => {
  // cs_themes.py stores Dark as org.x.apps.portal color-scheme 1 and the
  // theme under org.cinnamon.desktop.interface; GNOME's key stays Adwaita.
  const keys = {
    "org.gnome.desktop.interface gtk-theme": "'Adwaita'",
    "org.cinnamon.desktop.interface gtk-theme": "'Mint-Y-Dark-Aqua'",
  };
  assert.equal(gtkTheme({ portal: 1, keys }), "Adwaita:dark");
});

test("a light preference from the portal is light, whatever the theme is called", () => {
  const keys = { "org.gnome.desktop.interface gtk-theme": "'Adwaita-dark'" };
  assert.equal(gtkTheme({ portal: 2, keys }), "Adwaita:light");
});

test("with no portal, GNOME's color-scheme key decides", () => {
  assert.equal(
    gtkTheme({ keys: { "org.gnome.desktop.interface color-scheme": "'prefer-dark'" } }),
    "Adwaita:dark",
  );
  assert.equal(
    gtkTheme({
      keys: {
        "org.gnome.desktop.interface color-scheme": "'prefer-light'",
        "org.gnome.desktop.interface gtk-theme": "'Adwaita-dark'",
      },
    }),
    "Adwaita:light",
  );
});

test("with no stated preference, a theme named dark is still dark, as it was", () => {
  // GNOME with a dark theme and color-scheme left at default.
  assert.equal(
    gtkTheme({
      portal: 0,
      keys: {
        "org.gnome.desktop.interface color-scheme": "'default'",
        "org.gnome.desktop.interface gtk-theme": "'Adwaita-dark'",
      },
    }),
    "Adwaita:dark",
  );
  // Cinnamon in Mixed mode (portal 0) with a dark window theme picked.
  assert.equal(
    gtkTheme({ portal: 0, keys: { "org.cinnamon.desktop.interface gtk-theme": "'Mint-Y-Dark'" } }),
    "Adwaita:dark",
  );
  assert.equal(
    gtkTheme({ portal: 0, keys: { "org.cinnamon.desktop.interface gtk-theme": "'Mint-Y-Aqua'" } }),
    "Adwaita:light",
  );
});

test("nothing to go by is light, and APPIMAGE_GTK_THEME still overrides", () => {
  assert.equal(gtkTheme({}), "Adwaita:light");
  assert.equal(gtkTheme({ portal: 1, env: { APPIMAGE_GTK_THEME: "Adwaita" } }), "Adwaita");
});

test("a built hook matches when it begins with the plugin's verbatim part", () => {
  const head = hookHead(plugin);
  assert.ok(hookMatches(`${head}export GSETTINGS_SCHEMA_DIR="$APPDIR//usr/share/glib-2.0/schemas"\n`, plugin));
  const shipped = head.replace(/^# The desktop[\s\S]*?^esac\n/m, `${SHIPPED_THEME_LINE}\n`);
  assert.notEqual(shipped, head);
  assert.ok(!hookMatches(shipped, plugin));
});

test("the bundler's tools directory follows dirs::cache_dir", () => {
  assert.equal(toolsDir({ HOME: "/home/runner" }), "/home/runner/.cache/tauri");
  assert.equal(toolsDir({ HOME: "/home/runner", XDG_CACHE_HOME: "/var/cache/r" }), "/var/cache/r/tauri");
  // A relative XDG_CACHE_HOME is ignored, as the dirs crate does.
  assert.equal(toolsDir({ HOME: "/home/runner", XDG_CACHE_HOME: "cache" }), "/home/runner/.cache/tauri");
  assert.throws(() => toolsDir({}));
});
