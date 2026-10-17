import { describe, expect, it } from "vitest";
import { MY_TAG_LIMIT, newMyTagName } from "./myTags";

describe("newMyTagName", () => {
  it("names a new tag as rekordbox's createNewItemName does", () => {
    expect(newMyTagName([])).toBe("My Comment");
    expect(newMyTagName(["Peak"])).toBe("My Comment");
    expect(newMyTagName(["My Comment"])).toBe("My Comment (1)");
    expect(newMyTagName(["My Comment (1)", "My Comment"])).toBe("My Comment (2)");
    expect(newMyTagName(["My Comment", "My Comment (2)"])).toBe("My Comment (1)");
    expect(newMyTagName(["my comment"])).toBe("My Comment");
  });

  it("caps a category at rekordbox's fifty", () => {
    expect(MY_TAG_LIMIT).toBe(50);
  });
});
