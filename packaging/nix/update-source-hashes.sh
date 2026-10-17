#!/usr/bin/env bash
# Recompute the source package's fixed-output hashes and write them to
# packaging/nix/pin.json. Run from anywhere in the repository with network
# access and Nix available.
#
# The hashes cannot be derived from files in this repository: the Cargo one is
# the NAR hash of an external git checkout, and the pnpm one is the hash of a
# store built from npm tarballs. Both are deterministic from Cargo.lock and
# pnpm-lock.yaml, so this script derives them from those lockfiles instead of
# asking a person to update them.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

pin="packaging/nix/pin.json"
test -f "$pin"
test -f Cargo.lock

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tmp="$work/pin.json"

# --- Cargo git dependencies -------------------------------------------------
# importCargoLock needs the NAR hash of every git dependency. Cargo.lock pins
# the URL and revision, so fetch those and hash the checkout that results.
cargo="$(jq -c '.source.cargoOutputHashes // {}' "$pin")"
while IFS=$'\t' read -r name version url rev; do
  [ -n "$name" ] || continue
  store="$(nix eval --impure --raw --expr \
    "builtins.fetchGit { url = \"$url\"; rev = \"$rev\"; }")"
  hash="$(nix hash path --sri "$store")"
  cargo="$(jq --arg k "$name-$version" --arg v "$hash" '. + {($k): $v}' <<<"$cargo")"
done < <(awk '
  BEGIN { RS = ""; FS = "\n" }
  {
    name = ""; version = ""; source = ""
    for (i = 1; i <= NF; i++) {
      if ($i ~ /^name = /)    { name = $i;    sub(/^name = "/, "", name);       sub(/"$/, "", name) }
      if ($i ~ /^version = /) { version = $i; sub(/^version = "/, "", version); sub(/"$/, "", version) }
      if ($i ~ /^source = "git\+/) {
        source = $i
        sub(/^source = "git\+/, "", source)
        sub(/"$/, "", source)
      }
    }
    if (source != "") {
      split(source, parts, "#")
      urlrev = parts[1]; rev = parts[2]
      sub(/\?.*/, "", urlrev)
      print name "\t" version "\t" urlrev "\t" rev
    }
  }
' Cargo.lock)

jq --argjson c "$cargo" '.source.cargoOutputHashes = $c' "$pin" > "$tmp"
mv "$tmp" "$pin"

# --- pnpm store -------------------------------------------------------------
# Build the store with the hash already pinned. If the lockfile moved, the
# build reports the hash it actually produced; take that. If it succeeds, the
# pin is current.
log="$work/pnpm.log"
if nix build --no-link --print-out-paths .#source.pnpmDeps >"$log" 2>&1; then
  echo "pnpm dependency hash is current"
elif grep -q "hash mismatch" "$log"; then
  got="$(sed -n 's/^ *got: *\(sha256-[A-Za-z0-9+/=]*\).*/\1/p' "$log" | tail -1)"
  test -n "$got"
  jq --arg h "$got" '.source.pnpmDepsHash = $h' "$pin" > "$tmp"
  mv "$tmp" "$pin"
  echo "updated pnpm dependency hash"
else
  cat "$log"
  exit 1
fi

# --- verify -----------------------------------------------------------------
nix build --no-link .#source
echo "source hashes in $pin are current"
