import { describe, expect, it } from "vitest";
import { errorMessage } from "./errorMessage";

describe("errorMessage", () => {
  it("includes the developer detail returned with an internal error", () => {
    expect(errorMessage({
      kind: "internal",
      message: "rbxport could not complete this action because it encountered an unexpected problem. Try again, and send a problem report if it keeps happening.",
      detail: "The backup folder is not writable: permission denied",
    })).toBe("rbxport could not complete this action because it encountered an unexpected problem. Try again, and send a problem report if it keeps happening. The backup folder is not writable: permission denied");
  });

  it("does not repeat identical detail", () => {
    expect(errorMessage({ message: "Could not sync.", detail: "Could not sync." })).toBe("Could not sync.");
  });

  it("preserves ordinary errors and unknown fallbacks", () => {
    expect(errorMessage(new Error("Network unavailable"))).toBe("Network unavailable");
    expect(errorMessage(null)).toBe("Something went wrong. Please try again.");
  });
});
