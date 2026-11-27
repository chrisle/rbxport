import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("version synchronization preserves independently versioned lockfile packages", t => {
  const root = mkdtempSync(join(tmpdir(), "rbxport-sync-version-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const write = (file, text) => writeFileSync(join(root, file), text);
  const read = file => readFileSync(join(root, file), "utf8");
  for (const dir of ["scripts", "src-tauri", "crates/rbl-example", "crates/independent"]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  copyFileSync(new URL("./sync-version.mjs", import.meta.url), join(root, "scripts/sync-version.mjs"));
  write("package.json", '{"version":"1.10.0"}\n');
  write("src-tauri/tauri.conf.json", '{"version":"1.10.0"}\n');
  write("Cargo.toml", '[workspace.package]\nversion = "1.10.0"\n');
  write("src-tauri/Cargo.toml", '[package]\nname = "rbxport"\nversion.workspace = true\n');
  write("crates/rbl-example/Cargo.toml", '[package]\nname = "rbl-example"\nversion.workspace = true\n');
  write("crates/independent/Cargo.toml", '[package]\nname = "independent"\nversion = "1.10.0"\n');
  const block = (name, version, source = "") =>
    `\n[[package]]\nname = "${name}"\nversion = "${version}"\n${source}`;
  const unchanged = [
    block("tauri-plugin-updater", "2.11.0"),
    block("independent", "1.10.0"),
    block("rbl-example", "0.2.0"),
    block("rbl-example", "1.10.0", 'source = "registry+https://example.test"\n'),
  ].join("");
  const lockfile = version => 'version = 4\n' + block("rbxport", version) + block("rbl-example", version) + unchanged;
  write("Cargo.lock", lockfile("1.10.0"));

  const sync = () => execFileSync(process.execPath, [join(root, "scripts/sync-version.mjs"), "--version", "1.11.0"], { encoding: "utf8" });
  sync();
  assert.equal(read("Cargo.lock"), lockfile("1.11.0"));
  assert.match(read("Cargo.toml"), /version = "1\.11\.0"/);
  for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
    assert.equal(JSON.parse(read(file)).version, "1.11.0");
  }
  assert.match(sync(), /\(unchanged\)/);
});
