/** Read-only ELF check for the Pango/HarfBuzz pair shipped in an AppImage. */
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, realpathSync } from "node:fs";
import { join, sep } from "node:path";

// GNU and LLVM readelf both support this stable, wide symbol-table format.
export function symbols(output) {
  return output.split("\n").flatMap((line) => {
    const match = /^\s*\d+:\s+\S+\s+\d+\s+\S+\s+(GLOBAL|WEAK)\s+(DEFAULT|PROTECTED)\s+(\S+)\s+(\S+)/.exec(line);
    return match ? [{ weak: match[1] === "WEAK", undefined: match[3] === "UND", name: match[4].split("@")[0] }] : [];
  });
}

export function checkHarfbuzz(provider, consumers) {
  const exports = new Set(provider.filter((s) => !s.undefined).map((s) => s.name));
  if (![...exports].some((name) => name.startsWith("hb_"))) throw new Error("bundled HarfBuzz has no exported hb_* symbols");
  for (const [file, table] of consumers) {
    if (file === "libpango-1.0.so.0" && !table.some((s) => s.undefined && s.name.startsWith("hb_"))) {
      throw new Error("cannot identify Pango HarfBuzz imports in the ELF symbol table");
    }
    const missing = table.filter((s) => s.undefined && !s.weak && s.name.startsWith("hb_") && !exports.has(s.name));
    if (missing.length) throw new Error(`${file}: bundled HarfBuzz does not provide ${missing.map((s) => s.name).join(", ")}`);
  }
}

export function verifyHarfbuzz(libdir, readelf = "readelf") {
  const provider = join(libdir, "libharfbuzz.so.0");
  if (!existsSync(provider)) throw new Error("AppImage is missing libharfbuzz.so.0; bundled Pango must not use the host's older HarfBuzz (#337)");
  const names = readdirSync(libdir).filter((name) => /^libpango.*\.so\.\d+$/.test(name) || name === "libharfbuzz-icu.so.0");
  if (!names.includes("libpango-1.0.so.0")) throw new Error("AppImage is missing its Pango consumer");
  const root = realpathSync(libdir);
  const contained = (file) => {
    const actual = realpathSync(file);
    if (!actual.startsWith(root + sep)) throw new Error(`${file}: library resolves outside the extracted AppImage library directory`);
    return actual;
  };
  // Validate every input before inspecting any ELF, including normal SONAME links.
  const providerFile = contained(provider);
  const consumerFiles = names.map((name) => [name, contained(join(libdir, name))]);
  const inspect = (file, flag) => execFileSync(readelf, ["--wide", flag, file], { encoding: "utf8", env: { ...process.env, LC_ALL: "C" } });
  const identity = (file) => {
    const header = inspect(file, "--file-header");
    return ["Class", "Data", "Machine"].map((key) => {
      const match = new RegExp(`^\\s*${key}:\\s*(.+)$`, "m").exec(header);
      if (!match) throw new Error(`${file}: invalid ELF header`);
      return match[1];
    }).join(";");
  };
  const providerIdentity = identity(providerFile);
  if (!/\(SONAME\)[^\n]*\[libharfbuzz\.so\.0\]/.test(inspect(providerFile, "--dynamic"))) {
    throw new Error("bundled HarfBuzz has the wrong SONAME");
  }
  const consumers = consumerFiles.map(([name, file]) => {
    if (identity(file) !== providerIdentity) throw new Error(`${name}: ELF architecture differs from bundled HarfBuzz`);
    return [name, symbols(inspect(file, "--dyn-syms"))];
  });
  checkHarfbuzz(symbols(inspect(providerFile, "--dyn-syms")), consumers);
  return names;
}
