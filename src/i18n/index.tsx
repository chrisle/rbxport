import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";

import { loadPreferences, type Locale } from "@/lib/preferences";

export const LANGUAGE_CHOICES: readonly { value: Locale; label: string }[] = [
  { value: "en", label: "English" }, { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" }, { value: "es", label: "Español" },
  { value: "it", label: "Italiano" }, { value: "nl", label: "Nederlands" },
  { value: "ru", label: "Русский" }, { value: "pt", label: "Português" },
  { value: "sv", label: "Svenska" }, { value: "da", label: "Dansk" },
  { value: "tr", label: "Türkçe" }, { value: "el", label: "Ελληνικά" },
  { value: "hu", label: "Magyar" }, { value: "cs", label: "čeština" },
  { value: "zh-CN", label: "简体中文" }, { value: "zh-TW", label: "繁體中文" },
  { value: "ko", label: "한국어" }, { value: "ja", label: "日本語" },
];

export type Catalog = Readonly<Record<string, string>>;
const originalText = new WeakMap<Text, string>();
const originalAttributes = new WeakMap<Element, Map<string, string>>();
const templateCache = new WeakMap<Catalog, readonly { pattern: RegExp; translated: string }[]>();
const attributes = ["aria-label", "placeholder", "title"] as const;

function storedLocale(): Locale {
  return loadPreferences().view.locale;
}

export async function loadCatalog(locale: Locale): Promise<Catalog> {
  if (locale === "en") return {};
  const response = await fetch(new URL(`locales/${locale}.json`, document.baseURI));
  if (!response.ok) throw new Error(`Could not load ${locale} translations`);
  return response.json() as Promise<Catalog>;
}

export function translate(text: string, catalog: Catalog): string {
  const exact = catalog[text];
  if (exact !== undefined) return exact;
  let templates = templateCache.get(catalog);
  if (!templates) {
    templates = Object.entries(catalog).flatMap(([source, translated]) => {
      if (!/\{[^}]+\}/.test(source)) return [];
      const pattern = source.split(/\{[^}]+\}/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("(.+?)");
      return [{ pattern: new RegExp(`^${pattern}$`), translated }];
    });
    templateCache.set(catalog, templates);
  }
  for (const template of templates) {
    const match = text.match(template.pattern);
    if (!match) continue;
    let index = 1;
    return template.translated.replace(/\{[^}]+\}/g, () => match[index++] ?? "");
  }
  return text;
}

const TranslationContext = createContext<Catalog>({});

export function useTranslation(): (text: string, values?: Readonly<Record<string, string | number>>) => string {
  const catalog = useContext(TranslationContext);
  return useCallback((text, values = {}) => {
    let result = translate(text, catalog);
    for (const [name, value] of Object.entries(values)) result = result.replaceAll(`{${name}}`, String(value));
    return result;
  }, [catalog]);
}

function localize(root: Node, catalog: Catalog, force: boolean) {
  const visit = (node: Text) => {
    const parent = node.parentElement;
    if (!parent || parent.closest("script,style,[contenteditable=true],[data-i18n-ignore]")) return;
    const current = node.data;
    let source = originalText.get(node);
    if (source === undefined || (!force && current !== translate(source, catalog))) {
      source = current;
      originalText.set(node, source);
    }
    if (source.trim() === "") return;
    const leading = source.match(/^\s*/)?.[0] ?? "";
    const trailing = source.match(/\s*$/)?.[0] ?? "";
    const core = source.slice(leading.length, source.length - trailing.length);
    const next = `${leading}${translate(core, catalog)}${trailing}`;
    if (node.data !== next) node.data = next;
  };
  const visitElement = (element: Element) => {
    if (element.hasAttribute("data-i18n-ignore")) return;
    let originals = originalAttributes.get(element);
    if (!originals) { originals = new Map(); originalAttributes.set(element, originals); }
    for (const name of attributes) {
      const current = element.getAttribute(name);
      if (current === null) continue;
      let source = originals.get(name);
      if (source === undefined || (!force && current !== translate(source, catalog))) {
        source = current;
        originals.set(name, source);
      }
      const next = translate(source, catalog);
      if (current !== next) element.setAttribute(name, next);
    }
  };
  if (root instanceof Text) visit(root);
  if (root instanceof Element) visitElement(root);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node instanceof Text) visit(node);
    else if (node instanceof Element) visitElement(node);
  }
}

export function Localization({ children }: { children: ReactNode }) {
  const [locale, setLocale] = useState(storedLocale);
  const [catalog, setCatalog] = useState<Catalog>({});
  useEffect(() => {
    const refresh = () => setLocale(storedLocale());
    window.addEventListener("storage", refresh);
    window.addEventListener("rbl-preferences", refresh);
    return () => {
      window.removeEventListener("storage", refresh);
      window.removeEventListener("rbl-preferences", refresh);
    };
  }, []);
  useEffect(() => {
    let current = true;
    void loadCatalog(locale).then((loaded) => { if (current) setCatalog(loaded); });
    return () => { current = false; };
  }, [locale]);
  useEffect(() => {
    document.documentElement.lang = locale;
    localize(document.body, catalog, true);
    const observer = new MutationObserver((records) => {
      for (const record of records) {
        if (record.type === "characterData") localize(record.target, catalog, false);
        else if (record.type === "attributes") localize(record.target, catalog, false);
        else record.addedNodes.forEach((node) => localize(node, catalog, false));
      }
    });
    observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: [...attributes] });
    return () => observer.disconnect();
  }, [catalog, locale]);
  return <TranslationContext.Provider value={catalog}>{children}</TranslationContext.Provider>;
}
