// Exact generated pre-overlap-fix images; never uses the optional physical-stick helper.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub const JOURNAL: &str = ".rbxport-publication";

pub fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
pub fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for (path, bytes) in tree(from) {
        put(&to.join(&path), &bytes);
        let modified = fs::metadata(from.join(&path)).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(to.join(path))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }
}
pub fn rel(path: &str) -> PathBuf {
    let path = Path::new(path.trim_start_matches('/'));
    assert!(
        path.components()
            .all(|p| matches!(p, std::path::Component::Normal(_)))
    );
    path.to_owned()
}

#[derive(serde::Deserialize)]
struct Data {
    format: u8,
    producer: String,
    blobs: BTreeMap<String, Vec<u8>>,
    cases: Vec<Captured>,
}
#[derive(serde::Deserialize)]
struct Captured {
    root: String,
    artwork: bool,
    #[serde(default)]
    unchanged_artwork: bool,
    before: BTreeMap<PathBuf, String>,
    retained: BTreeMap<PathBuf, CapturedFile>,
}
#[derive(serde::Deserialize)]
struct CapturedFile {
    blob: String,
    modified_ns: String,
}
fn data() -> &'static Data {
    static DATA: std::sync::OnceLock<Data> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        use std::io::Read as _;
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/legacy_overlap_v1_9.json.gz"
            ))[..],
        )
        .take(16 * 1024 * 1024)
        .read_to_end(&mut raw)
        .unwrap();
        let data: Data = serde_json::from_slice(&raw).unwrap();
        assert_eq!(data.format, 1);
        assert_eq!(data.producer, "7fb4dad3f88a2cc0cecec0e718b4dde408fd7448");
        data
    })
}
fn captured_kind(mode: &str, artwork: bool, unchanged: bool) -> &'static Captured {
    data()
        .cases
        .iter()
        .find(|case| {
            case.root == mode && case.artwork == artwork && case.unchanged_artwork == unchanged
        })
        .unwrap()
}
pub fn before_kind(mode: &str, artwork: bool, unchanged: bool) -> BTreeMap<PathBuf, Vec<u8>> {
    captured_kind(mode, artwork, unchanged)
        .before
        .iter()
        .map(|(path, blob)| (path.clone(), data().blobs[blob].clone()))
        .collect()
}
pub fn before_stamps_kind(
    mode: &str,
    artwork: bool,
    unchanged: bool,
) -> BTreeMap<PathBuf, std::time::SystemTime> {
    let case = captured_kind(mode, artwork, unchanged);
    case.before
        .iter()
        .map(|(path, blob)| {
            let prior = Path::new(JOURNAL).join(".previous").join(path);
            let file = case
                .retained
                .get(&prior)
                .filter(|file| file.blob == *blob)
                .or_else(|| case.retained.get(path).filter(|file| file.blob == *blob))
                .unwrap();
            let nanos: u128 = file.modified_ns.parse().unwrap();
            (
                path.clone(),
                std::time::UNIX_EPOCH
                    + std::time::Duration::new(
                        u64::try_from(nanos / 1_000_000_000).unwrap(),
                        (nanos % 1_000_000_000) as u32,
                    ),
            )
        })
        .collect()
}

pub struct Fixture {
    _owned: tempfile::TempDir,
    path: PathBuf,
}
impl std::ops::Deref for Fixture {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
pub fn image_kind(mode: &str, artwork: bool, unchanged: bool) -> Fixture {
    let owned = tempfile::Builder::new()
        .prefix("rbxport-641-old-capture-")
        .tempdir()
        .unwrap();
    let path = owned.path().join("captured");
    for (name, file) in &captured_kind(mode, artwork, unchanged).retained {
        rel(name.to_str().unwrap());
        let target = path.join(name);
        put(&target, &data().blobs[&file.blob]);
        let nanos: u128 = file.modified_ns.parse().unwrap();
        let time = std::time::UNIX_EPOCH
            + std::time::Duration::new(
                u64::try_from(nanos / 1_000_000_000).unwrap(),
                (nanos % 1_000_000_000) as u32,
            );
        fs::File::options()
            .write(true)
            .open(&target)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(time))
            .unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().modified().unwrap(),
            time,
            "fixture requires exact captured timestamp precision"
        );
    }
    put(
        &owned.path().join("before.json"),
        &serde_json::to_vec(&before_kind(mode, artwork, unchanged)).unwrap(),
    );
    Fixture {
        _owned: owned,
        path,
    }
}
pub fn stamps(root: &Path) -> BTreeMap<PathBuf, std::time::SystemTime> {
    tree(root)
        .into_keys()
        .map(|p| {
            let modified = fs::metadata(root.join(&p)).unwrap().modified().unwrap();
            (p, modified)
        })
        .collect()
}
pub fn copy_owned(image: &Path) -> (tempfile::TempDir, PathBuf) {
    let owned = tempfile::Builder::new()
        .prefix("rbxport-641-recovery-")
        .tempdir()
        .unwrap();
    let root = owned.path().join("device-copy");
    copy_tree(image, &root);
    assert_eq!(tree(image), tree(&root));
    assert_eq!(stamps(image), stamps(&root));
    (owned, root)
}

pub fn image(mode: &str, artwork: bool) -> Fixture {
    image_kind(mode, artwork, false)
}
pub fn before_tree(mode: &str, artwork: bool) -> BTreeMap<PathBuf, Vec<u8>> {
    before_kind(mode, artwork, false)
}
pub fn before_stamps(mode: &str, artwork: bool) -> BTreeMap<PathBuf, std::time::SystemTime> {
    before_stamps_kind(mode, artwork, false)
}
