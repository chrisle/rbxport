/**
 * Builds src/i18n/ui.json and public/locales/*.json from UI strings and
 * rekordbox locale files, retaining existing translations and applying overrides.
 * Run: pnpm locales [LOCALE_DIRECTORY] [--translate-missing]. The default source
 * is the installed rekordbox 7 locale directory; --translate-missing uses Google
 * Translate over the network for missing entries.
 */
import { readFile, writeFile, mkdir, readdir } from "node:fs/promises";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = process.argv.slice(2).find((argument) => !argument.startsWith("--")) ??
  "/Applications/rekordbox 7/rekordbox.app/Contents/Resources/locale";
const outputRoot = resolve(root, "public/locales");
const translateMissing = process.argv.includes("--translate-missing");

const locales = {
  fr: "french.lang", de: "german.lang", es: "spanish.lang", it: "italian.lang",
  nl: "dutch.lang", ru: "russian.lang", pt: "portugal.lang", sv: "swedish.lang",
  da: "danish.lang", tr: "turkish.lang", el: "greek.lang", hu: "hungarian.lang",
  cs: "czech.lang", "zh-CN": "chinese_simplified.lang", "zh-TW": "chinese_traditional.lang",
  ko: "korean.lang", ja: "japanese.lang",
};

// Product terminology that should differ from rekordbox's untranslated loanwords
// or from a generic machine translation. Machine translation also tends to
// give singular keys a plural form; those corrections live here too.
const overrides = {
  cs: {
    "{size} total · {count} track": "{size} celkem · {count} skladba",
  },
  da: {
    "{size} total · {count} track": "{size} i alt · {count} nummer",
  },
  de: {
    "Backups": "Sicherungen",
    "Check for updates": "Nach Updates suchen",
    "No backups yet.": "Noch keine Sicherungen.",
    "Backups unavailable": "Sicherungen nicht verfügbar",
    "{count} minute ago": "vor {count} Minute",
    "{count} minutes ago": "vor {count} Minuten",
    "{count} hour ago": "vor {count} Stunde",
    "{count} hours ago": "vor {count} Stunden",
    "{count} day ago": "vor {count} Tag",
    "{count} days ago": "vor {count} Tagen",
    "{size} total · {count} track": "{size} insgesamt · {count} Titel",
  },
  es: {
    "{count} minute ago": "hace {count} minuto",
    "{count} minutes ago": "hace {count} minutos",
    "{count} hour ago": "hace {count} hora",
    "{count} hours ago": "hace {count} horas",
    "{count} day ago": "hace {count} día",
    "{count} days ago": "hace {count} días",
  },
  fr: {
    "{count} minute ago": "il y a {count} minute",
    "{count} minutes ago": "il y a {count} minutes",
    "{count} hour ago": "il y a {count} heure",
    "{count} hours ago": "il y a {count} heures",
    "{count} day ago": "il y a {count} jour",
    "{count} days ago": "il y a {count} jours",
  },
  ja: {
    "{count} minute ago": "{count} 分前",
  },
  pt: {
    "{size} total · {count} track": "{size} total · {count} faixa",
  },
  ru: {
    "{size} total · {count} track": "Всего {size} · {count} трек",
  },
  tr: {
    "{size} total · {count} track": "toplam {size} · {count} parça",
  },
  "zh-CN": {
    "{size} total · {count} track": "总计 {size} · {count} 首曲目",
  },
  "zh-TW": {
    "{size} total · {count} track": "總計 {size} · {count} 首曲目",
  },
};

function values(value, out = new Set()) {
  if (typeof value === "string") out.add(value);
  else if (value && typeof value === "object") Object.values(value).forEach((item) => values(item, out));
  return out;
}

async function sourceFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return entry.name === "locales" ? [] : sourceFiles(path);
    return /\.(ts|tsx)$/.test(entry.name) && !/\.test\.(ts|tsx)$/.test(entry.name) ? [path] : [];
  }));
  return nested.flat();
}

async function rustErrorStrings() {
  const found = new Set();
  const walk = async (directory) => {
    const entries = await readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        if (!["examples", "tests", "vendor"].includes(entry.name)) await walk(path);
        continue;
      }
      if (extname(entry.name) !== ".rs") continue;
      for (const line of (await readFile(path, "utf8")).split(/\r?\n/)) {
        if (!/(?:AppError|Conflict|Err\(|error\(|updater_error)/.test(line)) continue;
        for (const match of line.matchAll(/"((?:\\.|[^"\\])*)"/g)) {
          const text = unescapeLang(match[1]);
          if (/^[A-Z][^/]*[ .!?]$/.test(text)) found.add(text);
        }
      }
    }
  };
  await walk(resolve(root, "src-tauri/src"));
  await walk(resolve(root, "crates"));
  return found;
}

async function sourceStrings() {
  const found = new Set();
  for (const path of await sourceFiles(resolve(root, "src"))) {
    const source = ts.createSourceFile(
      path,
      await readFile(path, "utf8"),
      ts.ScriptTarget.Latest,
      true,
      extname(path) === ".tsx" ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
    );
    const visit = (node) => {
      if (ts.isStringLiteralLike(node)) found.add(node.text);
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return found;
}

const uiProperties = new Set([
  "aria-label", "caption", "description", "emptyText", "heading", "label", "message",
  "placeholder", "text", "title", "tooltip",
]);

const hiddenAttributes = new Set(["key", "className", "id", "role", "type", "htmlFor", "name", "autoComplete", "inputMode"]);
function hiddenAttribute(name) {
  return hiddenAttributes.has(name) || name.startsWith("data-") || (name.startsWith("aria-") && !uiProperties.has(name));
}

function cleanUiText(text) {
  return text
    .replaceAll("&hellip;", "…").replaceAll("&rsquo;", "’").replaceAll("&apos;", "'")
    .replaceAll("&quot;", "\"").replaceAll("&amp;", "&")
    .replace(/\s+/g, " ").trim();
}

async function uiStrings() {
  const found = new Set();
  const add = (text) => {
    const cleaned = cleanUiText(text);
    if (cleaned && /[A-Za-z\p{L}]/u.test(cleaned)) found.add(cleaned);
  };
  for (const path of await sourceFiles(resolve(root, "src"))) {
    const source = ts.createSourceFile(
      path, await readFile(path, "utf8"), ts.ScriptTarget.Latest, true,
      extname(path) === ".tsx" ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
    );
    const hasAncestor = (node, predicate) => {
      for (let parent = node.parent; parent; parent = parent.parent) {
        if (predicate(parent)) return true;
        if (ts.isSourceFile(parent) || ts.isFunctionLike(parent)) return false;
      }
      return false;
    };
    const inUiVariable = (node) => hasAncestor(node, (parent) =>
      ts.isVariableDeclaration(parent) && /(?:error|hint|message|notice|status|summary|text)$/i.test(parent.name.getText(source))
    );
    const inUiCall = (node) => hasAncestor(node, (parent) => {
      if (ts.isNewExpression(parent) && parent.expression.getText(source) === "Error") return true;
      if (!ts.isCallExpression(parent)) return false;
      const name = parent.expression.getText(source);
      return /(?:confirm|set(?:\w*(?:Error|Message|Status))|t)$/.test(name);
    });
    // A literal compared against, switched on, or handed to a DOM attribute
    // that is never shown (key, className, role, data-*) is an identifier, not
    // text. Component props are left alone: they are often text shown later.
    const isIdentifier = (node) => {
      const parent = node.parent;
      if (ts.isCaseClause(parent)) return true;
      if (ts.isBinaryExpression(parent) && [
        ts.SyntaxKind.EqualsEqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsEqualsToken,
        ts.SyntaxKind.EqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsToken,
      ].includes(parent.operatorToken.kind)) return true;
      for (let ancestor = parent; ancestor; ancestor = ancestor.parent) {
        if (ts.isJsxAttribute(ancestor)) return hiddenAttribute(ancestor.name.getText(source));
        if (ts.isSourceFile(ancestor) || ts.isFunctionLike(ancestor)) return false;
      }
      return false;
    };
    const visit = (node) => {
      if (ts.isJsxText(node)) add(node.text);
      if (ts.isJsxAttribute(node) && uiProperties.has(node.name.getText(source)) && node.initializer) {
        if (ts.isStringLiteral(node.initializer)) add(node.initializer.text);
        else if (ts.isJsxExpression(node.initializer) && node.initializer.expression &&
          ts.isStringLiteralLike(node.initializer.expression)) add(node.initializer.expression.text);
      }
      if (ts.isPropertyAssignment(node) &&
        uiProperties.has(node.name.getText(source).replaceAll(/["']/g, "")) &&
        ts.isStringLiteralLike(node.initializer)) add(node.initializer.text);
      if (ts.isStringLiteralLike(node) && !isIdentifier(node) && (
        hasAncestor(node, ts.isJsxExpression) || inUiVariable(node) || inUiCall(node)
      )) add(node.text);
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return found;
}

async function machineTranslations(strings, locale) {
  const boundary = "\n__RBXPORT_STRING_BOUNDARY_8E41__\n";
  const chunks = [];
  let chunk = [];
  let length = 0;
  for (const value of strings) {
    if (length + value.length + boundary.length > 3_500 && chunk.length) {
      chunks.push(chunk); chunk = []; length = 0;
    }
    chunk.push(value); length += value.length + boundary.length;
  }
  if (chunk.length) chunks.push(chunk);
  const translated = new Map();
  for (const values of chunks) {
    const url = new URL("https://translate.googleapis.com/translate_a/single");
    for (const [key, value] of Object.entries({ client: "gtx", sl: "en", tl: locale, dt: "t", q: values.join(boundary) })) {
      url.searchParams.set(key, value);
    }
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Translation failed for ${locale}: ${response.status}`);
    const payload = await response.json();
    const output = payload[0].map((part) => part[0]).join("");
    const results = output.split(boundary);
    if (results.length !== values.length) throw new Error(`Translation boundary lost for ${locale}`);
    values.forEach((value, index) => translated.set(value, results[index]));
  }
  return translated;
}

function unescapeLang(value) {
  return value.replaceAll("\\n", "\n").replaceAll("\\\"", "\"").replaceAll("\\\\", "\\");
}

function restorePlaceholders(source, translated) {
  const placeholders = source.match(/\{[a-z][a-z0-9]*\}/gi) ?? [];
  let index = 0;
  return translated.replace(/\{[^}]+\}/g, () => placeholders[index++] ?? "");
}

const english = JSON.parse(await readFile(resolve(root, "src/i18n/en.json"), "utf8"));
const wanted = new Set([...values(english), ...await sourceStrings()]);
const ui = new Set([...await uiStrings(), ...await rustErrorStrings()]);
await mkdir(outputRoot, { recursive: true });
await writeFile(resolve(root, "src/i18n/ui.json"), `${JSON.stringify([...ui].sort(), null, 2)}\n`);

for (const [locale, filename] of Object.entries(locales)) {
  const source = await readFile(resolve(sourceRoot, filename), "utf8");
  const available = new Map();
  for (const line of source.split(/\r?\n/)) {
    const match = line.match(/^\s*"((?:\\.|[^"\\])*)"\s*=\s*"((?:\\.|[^"\\])*)"/);
    if (!match) continue;
    const key = unescapeLang(match[1]);
    const translated = unescapeLang(match[2]);
    if (translated && !available.has(key)) available.set(key, translated);
  }
  let previous = {};
  try { previous = JSON.parse(await readFile(resolve(outputRoot, `${locale}.json`), "utf8")); } catch { /* first run */ }
  const missing = [...ui].filter((key) => !available.get(key) && previous[key] === undefined);
  const generated = translateMissing ? await machineTranslations(missing, locale) : new Map();
  if (missing.length && !translateMissing) {
    throw new Error(`${locale} is missing ${missing.length} UI translations; rerun with --translate-missing`);
  }
  const catalog = Object.fromEntries([...wanted].flatMap((key) => {
    const translated = available.get(key);
    return translated && translated !== key ? [[key, translated]] : [];
  }));
  for (const key of ui) {
    const translated = available.get(key) ?? previous[key] ?? generated.get(key) ?? key;
    catalog[key] = restorePlaceholders(key, translated);
  }
  Object.assign(catalog, overrides[locale] ?? {});
  await writeFile(resolve(outputRoot, `${locale}.json`), `${JSON.stringify(catalog, null, 2)}\n`);
  console.log(`${locale}: ${Object.keys(catalog).length} entries, ${ui.size}/${ui.size} UI strings`);
}
