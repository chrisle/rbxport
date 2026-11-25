/**
 * @vitest-environment jsdom
 *
 * The limiter's controls on the Audio pane: what the switch and the sliders
 * send up, when the sliders are usable, and what the readouts say. The
 * device list is a backend call the pane makes on its own; here the backend
 * has nothing to offer, which is the build-without-an-engine case.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { AudioDevices, Backend, Limiter } from "@/ipc/types";
import { DEFAULT_PREFERENCES, PREFERENCES_KEY } from "@/lib/preferences";
import { DEFAULT_LIMITER } from "@/store/useLimiter";
import { PreferencesProvider, usePreferencesStore, type PreferencesStore } from "@/store/usePreferences";
import { useAudioOutput } from "@/store/useAudioOutput";
import { AudioPane } from "./AudioPane";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let onLimiterChange: ReturnType<typeof vi.fn>;

function mount(limiter: Limiter, reduction = 0, peakLeft = 0, peakRight = 0) {
  act(() => {
    root.render(
      <AudioPane
        tab="configuration"
        limiter={limiter}
        onLimiterChange={onLimiterChange}
        reduction={reduction}
        peakLeft={peakLeft}
        peakRight={peakRight}
      />,
    );
  });
}

const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

const toggle = () => host.querySelector<HTMLInputElement>('input[role="switch"]');
const slider = (label: string) => host.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);

/**
 * Moves a range input the way a drag does: set the value, then fire input.
 * Through `valueAsNumber` rather than `value`, which React wraps on the
 * element to tell its own writes from the user's.
 */
function drag(input: HTMLInputElement, value: number) {
  act(() => {
    input.valueAsNumber = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  onLimiterChange = vi.fn();
  __setBackend({
    audioDevices: () => Promise.resolve({ devices: [], default: null, chosen: null }),
  } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("AudioPane's limiter controls", () => {
  it("shows the ceiling and release as the window writes them", async () => {
    mount(DEFAULT_LIMITER);
    await settle();
    expect(host.textContent).toContain("Ceiling 0.0 dB");
    expect(host.textContent).toContain("Release 250 ms");
    expect(toggle()?.checked).toBe(false);
    expect(slider("Limiter ceiling")?.disabled).toBe(true);
    expect(slider("Limiter release")?.disabled).toBe(true);
  });

  it("the switch sends the new enabled state alone", async () => {
    mount(DEFAULT_LIMITER);
    await settle();
    act(() => toggle()?.click());
    expect(onLimiterChange).toHaveBeenCalledWith({ enabled: true });
    expect(onLimiterChange).toHaveBeenCalledTimes(1);
  });

  it("the sliders send their number alone, in the engine's units", async () => {
    mount({ ...DEFAULT_LIMITER, enabled: true });
    await settle();
    const ceiling = slider("Limiter ceiling");
    const release = slider("Limiter release");
    if (!ceiling || !release) throw new Error("no sliders");
    expect([ceiling.min, ceiling.max, ceiling.step]).toEqual(["-12", "0", "0.1"]);
    expect([release.min, release.max, release.step]).toEqual(["10", "1000", "10"]);
    const input = slider("Limiter input gain");
    if (!input) throw new Error("no input gain slider");
    expect([input.min, input.max, input.step]).toEqual(["-24", "24", "0.1"]);
    drag(input, 6);
    expect(onLimiterChange).toHaveBeenLastCalledWith({ inputGainDb: 6 });
    drag(ceiling, -3);
    expect(onLimiterChange).toHaveBeenLastCalledWith({ ceilingDb: -3 });
    drag(release, 300);
    expect(onLimiterChange).toHaveBeenLastCalledWith({ releaseMs: 300 });
  });

  it("greys the sliders out while the limiter is off, but still shows the numbers", async () => {
    mount({ inputGainDb: 6, enabled: false, ceilingDb: -6, releaseMs: 500 });
    await settle();
    expect(toggle()?.checked).toBe(false);
    expect(slider("Limiter ceiling")?.disabled).toBe(true);
    expect(slider("Limiter release")?.disabled).toBe(true);
    expect(host.textContent).toContain("Ceiling -6.0 dB");
    expect(host.textContent).toContain("Release 500 ms");
    act(() => toggle()?.click());
    expect(onLimiterChange).toHaveBeenCalledWith({ enabled: true });
  });

  it("meters the output per channel and the reduction, each with its figure", async () => {
    mount({ ...DEFAULT_LIMITER, enabled: true }, 2.46, 0.5, 1);
    await settle();
    const meters = host.querySelectorAll<HTMLElement>("[role=meter]");
    expect([...meters].map((meter) => meter.getAttribute("aria-label"))).toEqual([
      "Output L",
      "Output R",
      "Gain reduction",
    ]);
    // Half is six decibels down: nine tenths of a sixty-decibel scale.
    expect(meters[0]?.getAttribute("aria-valuenow")).toBe("90");
    expect(meters[1]?.getAttribute("aria-valuenow")).toBe("100");
    expect(host.textContent).toContain("-6.0 dB");
    expect(host.textContent).toContain("0.0 dB");
    expect(host.textContent).toContain("−2.5 dB");
    // Reduction is on a twelve-decibel scale.
    expect(meters[2]?.getAttribute("aria-valuenow")).toBe("2.46");

    mount(DEFAULT_LIMITER, 0);
    expect(host.textContent).toContain("−∞ dB");
    expect(host.textContent).toContain("0.0 dB");
  });
});

it("reset restores ceiling and release without switching the limiter off", async () => {
  mount({ inputGainDb: 6, enabled: true, ceilingDb: -2, releaseMs: 500 });
  await settle();
  const reset = [...host.querySelectorAll("button")].find(button => button.textContent === "Reset settings");
  act(() => reset?.click());
  expect(onLimiterChange).toHaveBeenCalledWith({ inputGainDb: DEFAULT_LIMITER.inputGainDb, ceilingDb: DEFAULT_LIMITER.ceilingDb, releaseMs: DEFAULT_LIMITER.releaseMs });
});

it("shows bypassed reduction as zero while continuing to meter output", async () => {
  mount(DEFAULT_LIMITER, 4, 0.5, 0.5);
  await settle();
  expect(host.querySelector('[aria-label="Gain reduction"]')?.getAttribute("aria-valuenow")).toBe("0");
  expect(host.querySelector('[aria-label="Gain reduction"]')?.getAttribute("aria-valuetext")).toBe("Limiter off");
});

describe("AudioPane's output device (#799)", () => {
  const devices = [
    { id: "a", name: "Built-in" },
    { id: "m4", name: "M4" },
  ];
  const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="Audio output device"]');

  function mountWithStore(outputDevice: string | null, update: PreferencesStore["update"]) {
    const store: PreferencesStore = {
      preferences: { ...DEFAULT_PREFERENCES, audio: { ...DEFAULT_PREFERENCES.audio, outputDevice } },
      update,
      reset: () => {},
    };
    act(() => {
      root.render(
        <PreferencesProvider value={store}>
          <AudioPane tab="configuration" limiter={DEFAULT_LIMITER} onLimiterChange={onLimiterChange} reduction={0} />
        </PreferencesProvider>,
      );
    });
  }

  function choose(value: string) {
    act(() => {
      const el = select();
      if (!el) throw new Error("no select");
      el.value = value;
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
  }

  it("stores the chosen device and sends it to the engine; System default stores null", async () => {
    const setAudioDevice = vi.fn(() => Promise.resolve());
    __setBackend({
      audioDevices: () => Promise.resolve({ devices, default: "a", chosen: null }),
      setAudioDevice,
    } as unknown as Backend);
    const update = vi.fn();
    mountWithStore(null, update);
    await settle();
    choose("m4");
    await settle();
    expect(update).toHaveBeenLastCalledWith("audio", { outputDevice: "m4" });
    expect(setAudioDevice).toHaveBeenLastCalledWith("m4");
    choose("");
    await settle();
    expect(update).toHaveBeenLastCalledWith("audio", { outputDevice: null });
    expect(setAudioDevice).toHaveBeenLastCalledWith(null);
  });

  it("shows the device the engine is on", async () => {
    __setBackend({
      audioDevices: () => Promise.resolve({ devices, default: "a", chosen: "m4" }),
    } as unknown as Backend);
    mountWithStore("m4", vi.fn());
    await settle();
    expect(select()?.value).toBe("m4");
    expect(host.textContent).not.toContain("not connected");
  });

  it("says so when the saved device is missing, and does not overwrite the saved choice", async () => {
    __setBackend({
      audioDevices: () => Promise.resolve({ devices, default: "a", chosen: "m4-gone" }),
    } as unknown as Backend);
    const update = vi.fn();
    mountWithStore("m4-gone", update);
    await settle();
    expect(select()?.value).toBe("");
    expect(host.textContent).toContain("not connected");
    expect(update).not.toHaveBeenCalled();
  });

  it.each(["reset", "storage"] as const)("shows the applied output after a live preference %s", async change => {
    const setAudioDevice = vi.fn(() => Promise.resolve());
    __setBackend({
      audioDevices: () => Promise.resolve({ devices, default: "a", chosen: "m4" }),
      setAudioDevice,
    } as unknown as Backend);
    let store: PreferencesStore | undefined;
    function LiveOutput() {
      const current = usePreferencesStore({ ...DEFAULT_PREFERENCES, audio: { ...DEFAULT_PREFERENCES.audio, outputDevice: "m4" } });
      store = current;
      useAudioOutput(current.preferences.audio.outputDevice);
      return <PreferencesProvider value={current}>
        <AudioPane tab="configuration" limiter={DEFAULT_LIMITER} onLimiterChange={onLimiterChange} reduction={0} />
      </PreferencesProvider>;
    }
    act(() => root.render(<LiveOutput />));
    await settle();
    expect(select()?.value).toBe("m4");
    act(() => {
      if (change === "reset") store?.reset("audio");
      else {
        localStorage.setItem(PREFERENCES_KEY, JSON.stringify(DEFAULT_PREFERENCES));
        window.dispatchEvent(new StorageEvent("storage", { key: PREFERENCES_KEY }));
      }
    });
    await settle();
    expect(setAudioDevice).toHaveBeenLastCalledWith(null);
    expect(JSON.parse(localStorage.getItem(PREFERENCES_KEY) ?? "{}").audio.outputDevice).toBeNull();
    expect(select()?.value).toBe("");
  });

  const available = (present = true): AudioDevices => ({
    devices: present ? devices : devices.slice(0, 1), default: "a", chosen: "m4",
  });
  function pendingDevices() {
    let resolve!: (value: AudioDevices) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<AudioDevices>((yes, no) => { resolve = yes; reject = no; });
    return { promise, resolve, reject };
  }
  const focus = () => act(() => { window.dispatchEvent(new Event("focus")); });

  it("refreshes vanished and returned saved devices on focus without changing routing intent", async () => {
    const audioDevices = vi.fn().mockResolvedValue(available());
    const setAudioDevice = vi.fn();
    __setBackend({ audioDevices, setAudioDevice } as unknown as Backend);
    const update = vi.fn();
    mountWithStore("m4", update);
    await settle();
    expect(select()?.value).toBe("m4");
    audioDevices.mockResolvedValue(available(false));
    focus(); await settle();
    expect(select()?.value).toBe("");
    expect(host.textContent).toContain("not connected");
    audioDevices.mockResolvedValue(available());
    focus(); await settle();
    expect(select()?.value).toBe("m4");
    expect(host.textContent).not.toContain("not connected");
    expect(update).not.toHaveBeenCalled();
    expect(setAudioDevice).not.toHaveBeenCalled();
  });

  it.each(["resolve", "reject"] as const)("ignores an older device scan's %s after a newer focus scan", async outcome => {
    const older = pendingDevices();
    const newer = pendingDevices();
    const audioDevices = vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    __setBackend({ audioDevices } as unknown as Backend);
    mountWithStore("m4", vi.fn());
    await settle();
    focus(); await settle();
    await act(async () => { newer.resolve(available(false)); await newer.promise; });
    expect(select()?.value).toBe("");
    if (outcome === "resolve") older.resolve(available());
    else older.reject(new Error("enumeration refused"));
    await settle();
    expect(select()?.value).toBe("");
    expect(host.textContent).toContain("not connected");
  });

  it("retains the last device list on transient failure and recovers from an initial failure", async () => {
    const audioDevices = vi.fn().mockRejectedValueOnce(new Error("not ready")).mockResolvedValueOnce(available())
      .mockRejectedValueOnce(new Error("temporary"));
    __setBackend({ audioDevices } as unknown as Backend);
    mountWithStore("m4", vi.fn()); await settle();
    expect(select()).toBeNull();
    expect(host.textContent).toContain("No output devices to choose from");
    focus(); await settle();
    expect(select()?.value).toBe("m4");
    focus(); await settle();
    expect(select()?.value).toBe("m4");
  });

  it("retires the listener and pending scan when the pane is disposed", async () => {
    const pending = pendingDevices();
    const audioDevices = vi.fn().mockReturnValue(pending.promise);
    __setBackend({ audioDevices } as unknown as Backend);
    mountWithStore("m4", vi.fn()); await settle();
    act(() => root.render(<div>another pane</div>));
    focus(); await settle();
    expect(audioDevices).toHaveBeenCalledTimes(1);
    pending.resolve(available()); await settle();
    expect(host.textContent).toBe("another pane");
  });

});
