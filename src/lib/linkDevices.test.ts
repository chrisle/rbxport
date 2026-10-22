import { describe, expect, it } from "vitest";
import { LINK_DEVICE_LIST_MAX_ID, listedLinkDevices } from "./linkDevices";

const device = (number: number, name = `device ${number}`) => ({ number, name });

describe("listedLinkDevices (issue #582)", () => {
  it("keeps ID 100 and hides ID 101", () => {
    expect(LINK_DEVICE_LIST_MAX_ID).toBe(100);
    const shown = listedLinkDevices([device(100), device(101)]);
    expect(shown.map((d) => d.number)).toEqual([100]);
  });

  it("hides TC-SHOWKONTROL when its ID is above 100", () => {
    const shown = listedLinkDevices([device(1), device(101, "TC-SHOWKONTROL"), device(2)]);
    expect(shown.map((d) => d.name)).toEqual(["device 1", "device 2"]);
  });

  it("keeps IDs below 100 and the order they arrived in", () => {
    const shown = listedLinkDevices([device(33), device(1), device(17), device(18)]);
    expect(shown.map((d) => d.number)).toEqual([33, 1, 17, 18]);
  });

  it("hides every ID above 100, up to the largest device number", () => {
    const shown = listedLinkDevices([device(99), device(100), device(102), device(255)]);
    expect(shown.map((d) => d.number)).toEqual([99, 100]);
  });

  it("returns an empty list when every device is above 100", () => {
    expect(listedLinkDevices([device(101), device(200)])).toEqual([]);
    expect(listedLinkDevices([])).toEqual([]);
  });
});
