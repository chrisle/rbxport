# RBXport Triode releases

The `triode` branch builds **RBXport Triode** for Apple Silicon. Its bundle
identifier is `com.rbxport.triode`, its native state lives under
`~/Library/Application Support/rbxport-triode`, and its updater reads
`https://download.rbxport.com/triode/latest.json`. The download URLs are public.

To release a new version, commit the changes, push `triode`, then create and
push a new tag from the commit you want to ship:

```sh
git push origin triode
git tag triode-v1.10.0
git push origin refs/tags/triode-v1.10.0
```

The initial Triode version matches `dev` at 1.10.0. Use a higher version on each subsequent Triode release. The workflow uses the tag's version
for the installer and feed. Local development displays the version checked
in to the branch. To set it, run `node scripts/sync-version.mjs --version VERSION`.

The [Release Triode workflow](../../.github/workflows/release-triode.yml)
uses the existing Apple and updater signing credentials and R2 bucket.
The repository variable `TRIODE_CARGO_TARGET_DIR` points to the build cache
on the Mac runner's Raid5 volume.
It publishes the DMG and signed update archive to a versioned path under
`triode/releases/` before updating the feed. Get the DMG URL from the feed's
`downloads.darwin-aarch64.url` field. Subsequent versions arrive through
Triode's built-in updater.
