/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import de from "../../public/locales/de.json";
import { Localization } from ".";

it("keeps literal own text and tooltip while translating ordinary and descendant control attributes", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.setItem("rbl.preferences", JSON.stringify({ view: { locale: "de" } }));
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: true, json: () => Promise.resolve(de) })));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(<Localization>
        <span data-testid="literal" data-i18n-ignore title="Play">Play</span>
        <button data-testid="ordinary" title="Play" aria-label="Play">Play</button>
        <div data-testid="wrapper" data-i18n-ignore title="Play">
          <button data-testid="descendant" title="Play" aria-label="Play">Play</button>
        </div>
      </Localization>);
      await Promise.resolve();
    });
    const element = (id: string) => host.querySelector(`[data-testid="${id}"]`)!;
    // The translated ordinary control proves catalogue application completed.
    await vi.waitFor(() => expect(element("ordinary").getAttribute("title")).toBe("Spielen"));
    expect(element("literal").textContent).toBe("Play");
    expect(element("literal").getAttribute("title")).toBe("Play");
    expect(element("wrapper").getAttribute("title")).toBe("Play");
    expect(element("ordinary").textContent).toBe("Spielen");
    expect(element("ordinary").getAttribute("aria-label")).toBe("Spielen");
    expect(element("descendant").textContent).toBe("Play");
    expect(element("descendant").getAttribute("title")).toBe("Spielen");
    expect(element("descendant").getAttribute("aria-label")).toBe("Spielen");
  } finally {
    await act(async () => { root.unmount(); await Promise.resolve(); });
    host.remove();
    localStorage.clear();
    vi.unstubAllGlobals();
  }
});
