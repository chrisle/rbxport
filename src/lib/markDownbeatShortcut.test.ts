import { describe, expect, it } from "vitest";
import { BINDINGS, dispatch, matchBinding } from "./shortcuts";
import { KEY_PRESETS, keyMap, presetChord } from "./keyPresets";
import { assignChord, rowsFor } from "@/views/settings/KeyboardPane";

describe("the existing downbeat button's assignable shortcut", () => {
  it("offers one app-owned, initially unbound row for each deck in every preset", () => {
    const bindings = BINDINGS.filter(binding => binding.action === "markDownbeat");
    expect(bindings).toHaveLength(2);
    for (const binding of bindings) {
      expect(binding.command).toBeUndefined();
      for (const preset of KEY_PRESETS) {
        expect(presetChord(binding, preset).chord.key).toBe("");
        expect(rowsFor(binding.deck === "a" ? "Player A" : "Player B", true, {}, preset))
          .toContainEqual(expect.objectContaining({ bindingId: binding.id, label: "Mark the downbeat here", built: true, key: null }));
      }
    }
  });
  it("assigns each deck independently and retains the typing guard", () => {
    let own = assignChord({}, "a.markDownbeat", { key: "F8" });
    own = assignChord(own, "b.markDownbeat", { key: "F8", shiftKey: true });
    const keys = keyMap({ preset: "export", overrides: own });
    expect(matchBinding({ key: "F8" }, { mac: true }, keys)).toMatchObject({ action: "markDownbeat", deck: "a" });
    expect(matchBinding({ key: "F8", shiftKey: true }, { mac: false }, keys)).toMatchObject({ action: "markDownbeat", deck: "b" });
    expect(dispatch({ key: "F8" }, { mac: true }, { tagName: "INPUT" }, keys)).toBeNull();
    expect(dispatch({ key: "F8" }, { mac: true }, { tagName: "TEXTAREA" }, keys)).toBeNull();
  });
});
