// Invoked only by the existing authorized installation shell. No application
// automation is used: the bridge calls libc, on the three supplied paths only.
function run(paths) {
    if (paths.length !== 3) throw new Error("Expected target, staged bundle and backup");
    ObjC.import("Foundation");
    ObjC.bindFunction("renamex_np", ["int", ["char *", "char *", "unsigned int"]]);
    ObjC.bindFunction("__error", ["int *", []]);
    var target = paths[0], staged = paths[1], backup = paths[2];
    function move(from, to) {
        // macOS RENAME_EXCL: refuse every existing destination, including an
        // empty directory or symlink, rather than overwrite or move inside it.
        return $.renamex_np(from, to, 4) === 0 ? 0 : $.__error()[0];
    }
    var saved = move(target, backup);
    if (saved) throw new Error("Could not back up the current app (errno " + saved + ")");
    var placement = move(staged, target);
    if (placement) {
        var restore = move(backup, target);
        throw new Error("Could not place the new app (errno " + placement + ")" +
            (restore ? "; previous app retained at " + backup + " (restore errno " + restore + ")" : "; previous app restored"));
    }
    // Cleanup stays in this authorized process: the previous bundle may be
    // root-owned. Failure here leaves a successful installation and a backup
    // whose location the Rust caller retains and logs.
    try {
        var cleanupError = Ref();
        if ($.NSFileManager.defaultManager.removeItemAtPathError($(backup), cleanupError)) return;
    } catch (error) {
        // Placement already succeeded; the caller checks the backup itself.
    }
    return "Update installed; previous app backup retained at " + backup;
}
