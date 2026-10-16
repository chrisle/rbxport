#!/usr/bin/env node

/**
 * Puts rbxport's copy of linuxdeploy-plugin-gtk where the Tauri bundler
 * looks for it, and checks that a built AppImage carries its AppRun hook.
 *
 * The hook picks the GTK theme the whole process runs with: it exports
 * `GTK_THEME=Adwaita:<variant>`, which GTK reads before any setting, so the
 * native menu bar is light or dark on what the hook decides alone (#448).
 * @tauri-apps/cli 2.11 downloads the plugin only when the file is missing
 * from its tools directory, and the copy cached on the build host predates
 * the hook reading the desktop's color-scheme preference: it went by whether
 * GNOME's theme name contained "dark", which Cinnamon never sets.
 *
 * Run:
 *   node scripts/appimage-gtk-hook.mjs install
 *   node scripts/appimage-gtk-hook.mjs verify <file.AppImage>
 */
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const PLUGIN = new URL("../packaging/appimage/linuxdeploy-plugin-gtk.sh", import.meta.url);
const PLUGIN_NAME = "linuxdeploy-plugin-gtk.sh";
const HOOK_PATH = "apprun-hooks/linuxdeploy-plugin-gtk.sh";

/**
 * The Tauri bundler's tools directory: `dirs::cache_dir()/tauri`, which is
 * `$XDG_CACHE_HOME` when it is an absolute path and `~/.cache` otherwise
 * (`bundle.useLocalToolsDir` is not set in tauri.conf.json).
 */
export function toolsDir(env) {
  const xdg = env.XDG_CACHE_HOME;
  if (xdg && isAbsolute(xdg)) return join(xdg, "tauri");
  if (!env.HOME) throw new Error("neither XDG_CACHE_HOME nor HOME is set");
  return join(env.HOME, ".cache", "tauri");
}

/** The part of the AppRun hook the plugin writes verbatim (its quoted heredoc). */
export function hookHead(plugin) {
  const match = /^cat > "\$HOOKFILE" <<\\EOF\n([\s\S]*?)^EOF$/m.exec(plugin);
  if (!match) throw new Error("the plugin has no quoted AppRun hook heredoc");
  return match[1];
}

/** Whether a hook built into an AppImage is the one this plugin writes. */
export function hookMatches(hook, plugin) {
  return hook.startsWith(hookHead(plugin));
}

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

function install(env) {
  const plugin = readFileSync(PLUGIN);
  const dir = toolsDir(env);
  const target = join(dir, PLUGIN_NAME);
  let previous = "none";
  try {
    previous = sha256(readFileSync(target));
  } catch {
    // Not cached yet.
  }
  mkdirSync(dir, { recursive: true });
  // Written beside and renamed, so a build on the same host never reads half.
  const staged = `${target}.${process.pid}.tmp`;
  writeFileSync(staged, plugin);
  chmodSync(staged, 0o755);
  renameSync(staged, target);
  console.log(`${target}: ${previous} -> ${sha256(plugin)}`);
}

function verify(appimage) {
  const plugin = readFileSync(PLUGIN, "utf8");
  const work = mkdtempSync(join(tmpdir(), "rbxport-448-hook-"));
  try {
    execFileSync(resolve(appimage), ["--appimage-extract", HOOK_PATH], { cwd: work, stdio: "ignore" });
    const hook = readFileSync(join(work, "squashfs-root", HOOK_PATH), "utf8");
    if (!hookMatches(hook, plugin)) {
      throw new Error(`${appimage}: its GTK AppRun hook is not the one in packaging/appimage (#448)`);
    }
    console.log(`${appimage}: GTK AppRun hook matches packaging/appimage`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, file] = process.argv.slice(2);
  if (command === "install") install(process.env);
  else if (command === "verify" && file) verify(file);
  else {
    console.error("usage: appimage-gtk-hook.mjs install | verify <file.AppImage>");
    process.exit(2);
  }
}
