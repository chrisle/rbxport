import { describe, expect, it } from "vitest";

import { formatBugReportAttachment, readReceipt } from "./bugReport";

describe("formatBugReportAttachment", () => {
  it("includes the current email and every supplied preference before the diagnostic log", () => {
    const preferences = {
      analysis: { auto: false, mode: "rbxport" },
      advanced: { protectLibrary: true, relocateFolders: ["/Music"] },
    };

    const attachment = formatBugReportAttachment(
      "System information\nrbxport 1.0.0\n\nApplication log\nready\n",
      "  dj@example.com  ",
      preferences,
    );

    const details = attachment.match(/^Report details\n([\s\S]+?)\n\nSystem information/)?.[1];
    expect(details).toBeDefined();
    expect(JSON.parse(details ?? "{}")).toEqual({ email: "dj@example.com", preferences });
    expect(attachment).toMatch(/Report details[\s\S]+System information[\s\S]+Application log/);
  });

  it("records that an optional email was not supplied", () => {
    expect(formatBugReportAttachment("log", "   ", {})).toContain('"email": null');
  });
});

describe("readReceipt", () => {
  it("keeps the issue URL of a report on the app's repository", () => {
    expect(readReceipt({ key: "#42", url: "https://github.com/chrisle/rbxport/issues/42", attachmentAdded: true }))
      .toEqual({ key: "#42", url: "https://github.com/chrisle/rbxport/issues/42", attachmentAdded: true });
  });

  it("accepts a receipt without a URL from an older report service", () => {
    expect(readReceipt({ key: "#42", attachmentAdded: false })).toEqual({ key: "#42", attachmentAdded: false });
  });

  it("drops a URL that is not an issue on the app's repository", () => {
    for (const url of [
      "https://example.com/chrisle/rbxport/issues/42",
      "http://github.com/chrisle/rbxport/issues/42",
      "https://github.com/someone/rbxport/issues/42",
      "https://github.com/chrisle/rbxport/issues/42/../../pulls",
      "https://github.com/chrisle/rbxport/issues/0",
      42,
    ]) {
      expect(readReceipt({ key: "#42", url, attachmentAdded: true })).toEqual({ key: "#42", attachmentAdded: true });
    }
  });

  it("rejects a response that is not a receipt", () => {
    expect(readReceipt(null)).toBeNull();
    expect(readReceipt({ url: "https://github.com/chrisle/rbxport/issues/42" })).toBeNull();
  });
});
