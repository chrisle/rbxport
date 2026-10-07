// A dedicated Worker; rbxport.com itself is static Cloudflare Pages content.
const REPORT_URL = "https://report.rbxport.com/";

export interface BugReportSubmission {
  email: string;
  description: string;
  attachment: string;
  turnstileToken: string;
}

export interface BugReportReceipt {
  key: string;
  /** The report's public issue page; absent from report services that predate it. */
  url?: string;
  attachmentAdded: boolean;
}

/** Only an issue on the app's own repository is shown to the reporter or opened. */
const ISSUE_URL = /^https:\/\/github\.com\/chrisle\/rbxport\/issues\/[1-9]\d*$/;

/** Add the reporter-provided identity and current, sanitised app settings. */
export function formatBugReportAttachment(base: string, email: string, preferences: object): string {
  const details = JSON.stringify({
    email: email.trim() || null,
    preferences,
  }, null, 2);
  return `Report details\n${details}\n\n${base}`;
}

function isReceipt(value: unknown): value is BugReportReceipt {
  return Boolean(value) && typeof value === "object" &&
    typeof (value as { key?: unknown }).key === "string" &&
    typeof (value as { attachmentAdded?: unknown }).attachmentAdded === "boolean";
}

/** The receipt as the app uses it: an issue URL it does not recognise is dropped. */
export function readReceipt(value: unknown): BugReportReceipt | null {
  if (!isReceipt(value)) return null;
  const url = (value as { url?: unknown }).url;
  return {
    key: value.key,
    attachmentAdded: value.attachmentAdded,
    ...(typeof url === "string" && ISSUE_URL.test(url) ? { url } : {}),
  };
}

export async function submitBugReport(submission: BugReportSubmission): Promise<BugReportReceipt> {
  const response = await fetch(REPORT_URL, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(submission),
  });
  const body: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const message = body && typeof body === "object" && "error" in body && typeof body.error === "string"
      ? body.error : "The report could not be submitted. Please try again later.";
    throw new Error(message);
  }
  const receipt = readReceipt(body);
  if (!receipt) {
    throw new Error("The report service returned an invalid response.");
  }
  return receipt;
}
