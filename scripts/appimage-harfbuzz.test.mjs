import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { checkHarfbuzz, symbols, verifyHarfbuzz } from "./appimage-harfbuzz.mjs";
import { PLUGIN } from "./appimage-gtk-hook.mjs";

const baseline = "hb_ot_layout_get_horizontal_baseline_tag_for_script";
const entry = (name, ndx = "UND", bind = "GLOBAL", vis = "DEFAULT") => `  12: 0000000000000000 0 FUNC ${bind} ${vis} ${ndx} ${name}`;

test("rejects old provider missing Pango's required baseline symbol (#337)", () => {
  const consumer = [["libpango-1.0.so.0", symbols(entry(baseline))]];
  assert.throws(() => checkHarfbuzz(symbols(entry("hb_shape", "12")), consumer), new RegExp(baseline));
  assert.doesNotThrow(() => checkHarfbuzz(symbols(entry(baseline, "12")), consumer));
});

test("refuses an empty or unrecognized symbol table instead of silently passing", () => {
  assert.throws(() => checkHarfbuzz([], [["libpango-1.0.so.0", symbols(entry(baseline))]]), /no exported/);
  assert.throws(() => checkHarfbuzz(symbols(entry(baseline, "12")), [["libpango-1.0.so.0", []]]), /cannot identify/);
});

test("checks optional ICU consumer, ignoring optional weak imports and hidden exports", () => {
  const imports = symbols([entry("hb_shape"), entry("hb_optional", "UND", "WEAK")].join("\n"));
  assert.doesNotThrow(() => checkHarfbuzz(symbols(entry("hb_shape", "12", "WEAK")), [["libharfbuzz-icu.so.0", imports]]));
  assert.throws(() => checkHarfbuzz(symbols(entry("hb_shape", "12", "GLOBAL", "HIDDEN")), [["libharfbuzz-icu.so.0", imports]]), /no exported hb_\* symbols/);
});

test("fails before inspection when release bundle omits main provider", () => {
  const dir = mkdtempSync(join(tmpdir(), "rbxport-337-missing-"));
  try { assert.throws(() => verifyHarfbuzz(dir), /missing libharfbuzz.so.0/); }
  finally { rmSync(dir, { recursive: true, force: true }); }
});

test("GTK plugin explicitly forces main HarfBuzz through linuxdeploy's exclusion list", () => {
  const dir = mkdtempSync(join(tmpdir(), "rbxport-337-plugin-"));
  try {
    const libdir = join(dir, "libs");
    mkdirSync(libdir);
    writeFileSync(join(libdir, "libharfbuzz.so.0"), "fixture");
    writeFileSync(join(libdir, "libharfbuzz-subset.so.0"), "unrelated");
    const plugin = readFileSync(PLUGIN, "utf8");
    const begin = plugin.indexOf('echo "Copying more libraries"');
    const end = plugin.indexOf("# Create symbolic links", begin);
    assert.ok(begin > 0 && end > begin);
    const script = join(dir, "collect.sh");
    writeFileSync(script, 'get_pkgconf_variable() { printf "%s" "$FIXTURE_LIBDIR"; }\ngdk_libdir="$FIXTURE_LIBDIR"\n' + plugin.slice(begin, end));
    // printf acts as linuxdeploy: output its exact invocation without loading ELF.
    const capture = join(dir, "capture.sh");
    writeFileSync(capture, '#!/bin/sh\nprintf "%s\\n" "$@"\n', { mode: 0o755 });
    const output = execFileSync("bash", [script], { encoding: "utf8", env: { ...process.env, FIXTURE_LIBDIR: libdir, LINUXDEPLOY: capture, APPDIR: dir } });
    assert.ok(output.includes(`--library=${join(libdir, "libharfbuzz.so.0")}\n`));
    assert.ok(!output.includes("--library=" + join(libdir, "libharfbuzz-subset.so.0")));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("bundle gate checks cairo/FT2/ICU consumers and ELF architecture/SONAME", () => {
  const dir = mkdtempSync(join(tmpdir(), "rbxport-337-elf-"));
  try {
    for (const name of ["libharfbuzz.so.0", "libpango-1.0.so.0", "libpangocairo-1.0.so.0", "libpangoft2-1.0.so.0", "libharfbuzz-icu.so.0"]) writeFileSync(join(dir, name), "");
    const tool = join(dir, "readelf");
    const run = ({ missing = false, wrongMachine = false, wrongSoname = false } = {}) => {
      writeFileSync(tool, `#!/bin/sh
case "$2" in
--file-header)
  echo '  Class: ELF64'
  echo '  Data: little endian'
  case "$3" in *libharfbuzz.so.0|*provider.elf) echo '  Machine: x86-64';; *) echo '  Machine: ${wrongMachine ? "AArch64" : "x86-64"}';; esac;;
--dynamic) echo '  0x0000000e (SONAME) Library soname: [${wrongSoname ? "other.so.0" : "libharfbuzz.so.0"}]';;
--dyn-syms)
  case "$3" in
  *libharfbuzz.so.0|*provider.elf) echo '${entry(baseline, "12")}';;
  *libpangoft2-1.0.so.0) echo '${entry(missing ? "hb_missing_ft2" : baseline)}';;
  *) echo '${entry(baseline)}';;
  esac;;
esac
`, { mode: 0o755 });
      return () => verifyHarfbuzz(dir, tool);
    };
    assert.equal(run()().length, 4);
    assert.throws(run({ missing: true }), /libpangoft2.*hb_missing_ft2/);
    assert.throws(run({ wrongMachine: true }), /architecture differs/);
    assert.throws(run({ wrongSoname: true }), /wrong SONAME/);
    unlinkSync(join(dir, "libharfbuzz.so.0"));
    writeFileSync(join(dir, "provider.elf"), "");
    symlinkSync("provider.elf", join(dir, "libharfbuzz.so.0"));
    assert.equal(run()().length, 4, "ordinary internal SONAME link remains supported");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});


test("refuses escaping provider or consumer links before ELF inspection", () => {
  const dir = mkdtempSync(join(tmpdir(), "rbxport-337-links-"));
  try {
    const libs = join(dir, "lib");
    mkdirSync(libs);
    const outside = join(dir, "host.so");
    writeFileSync(outside, "outside sentinel");
    writeFileSync(join(libs, "libpango-1.0.so.0"), "fixture");
    const provider = join(libs, "libharfbuzz.so.0");
    symlinkSync(outside, provider);
    assert.throws(() => verifyHarfbuzz(libs, "must-not-execute-readelf"), /resolves outside/);
    unlinkSync(provider);
    writeFileSync(provider, "fixture");
    unlinkSync(join(libs, "libpango-1.0.so.0"));
    symlinkSync("../host.so", join(libs, "libpango-1.0.so.0"));
    assert.throws(() => verifyHarfbuzz(libs, "must-not-execute-readelf"), /resolves outside/);
    assert.equal(readFileSync(outside, "utf8"), "outside sentinel");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
