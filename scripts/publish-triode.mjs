// Publish a signed Apple Silicon build and its updater manifest to Triode's R2 prefix.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const {
  BUCKET: bucket, ENDPOINT: endpoint, RELEASE_VERSION: version, SOURCE_REF: commit,
  GITHUB_RUN_ID: runId, GITHUB_RUN_ATTEMPT: attempt,
} = process.env;
const stableVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
if (!bucket || !endpoint || !stableVersion.test(version ?? "") ||
    !/^[a-f0-9]{40}$/.test(commit ?? "") ||
    !/^\d+$/.test(runId ?? "") || !/^\d+$/.test(attempt ?? "")) {
  throw new Error("Missing or invalid Triode publish configuration");
}

const origin = "https://download.rbxport.com";
const feedKey = "triode/latest.json";
const slug = "darwin-aarch64";
const buildPath = `triode/releases/${version}/${runId}-${attempt}`;
const releaseFile = (ext) => `rbxport-triode-${version}-${slug}.${ext}`;
const localFile = (ext) => `out/rbxport-${version}-${slug}.${ext}`;
const releaseUrl = (ext) => `${origin}/${buildPath}/${releaseFile(ext)}`;
const curl = (...args) => execFileSync("curl", [
  "--fail", "--silent", "--show-error", "--retry", "5", "--retry-all-errors",
  "--retry-delay", "5", "--max-time", "20", ...args,
], { encoding: "utf8" });
const aws = (...args) => execFileSync("aws", [
  "--endpoint-url", endpoint, "--no-cli-pager", ...args,
], { encoding: "utf8" });

function upload(file, key, cache, ...options) {
  aws("s3", "cp", file, `s3://${bucket}/${key}`, "--no-progress", "--cache-control", cache, ...options);
}

function compareVersions(left, right) {
  if (!stableVersion.test(right)) throw new Error("Invalid version in existing Triode feed");
  const incoming = left.split(".").map(BigInt);
  const published = right.split(".").map(BigInt);
  for (const [index, part] of incoming.entries()) {
    if (part > published[index]) return 1;
    if (part < published[index]) return -1;
  }
  return 0;
}

function publish(feedPath) {
  const listing = JSON.parse(aws("s3api", "list-objects-v2", "--bucket", bucket, "--prefix", feedKey, "--output", "json"));
  if (listing.Contents?.some(({ Key }) => Key === feedKey)) {
    aws("s3api", "get-object", "--bucket", bucket, "--key", feedKey, feedPath);
    const published = JSON.parse(readFileSync(feedPath, "utf8"));
    if (typeof published.version !== "string") throw new Error("Missing version in existing Triode feed");
    const order = compareVersions(version, published.version);
    if (order === 0 && published.source_commit === commit) return;
    if (order <= 0) throw new Error(`Triode ${version} must be newer than ${published.version}`);
  }

  for (const ext of ["app.tar.gz", "app.tar.gz.sig", "dmg"]) {
    const file = statSync(localFile(ext));
    if (!file.isFile() || file.size === 0) throw new Error(`Missing or empty ${localFile(ext)}`);
  }
  const signature = readFileSync(localFile("app.tar.gz.sig"), "utf8").trim();
  if (!signature) throw new Error("Missing updater signature");

  for (const ext of ["app.tar.gz", "app.tar.gz.sig", "dmg"]) {
    upload(localFile(ext), `${buildPath}/${releaseFile(ext)}`, "public, max-age=31536000, immutable");
    curl("--head", releaseUrl(ext));
  }
  const feed = {
    version,
    source_commit: commit,
    pub_date: new Date().toISOString(),
    platforms: { [slug]: { url: releaseUrl("app.tar.gz"), signature } },
    downloads: { [slug]: { file: releaseFile("dmg"), url: releaseUrl("dmg") } },
  };
  writeFileSync(feedPath, JSON.stringify(feed, null, 2) + "\n");
  upload(feedPath, feedKey, "public, max-age=60", "--content-type", "application/json");
}

const temp = mkdtempSync(join(tmpdir(), "triode-release-"));
try {
  const feedPath = join(temp, "latest.json");
  publish(feedPath);
  const actual = curl(`${origin}/${feedKey}?release=${runId}-${attempt}`);
  if (actual !== readFileSync(feedPath, "utf8")) throw new Error("Published Triode feed differs from R2");
  const feed = JSON.parse(actual);
  console.log(`Published Triode ${version}: ${feed.downloads[slug].url}`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
