//! What a selected device's tabs read and write on the stick.
//!
//! Two files hold everything the tabs can change:
//!
//! - `PIONEER/DEVSETTING.DAT` — the display settings a player reads: waveform
//!   colour, current-position marker, overview waveform type, key display.
//! - `PIONEER/rekordbox/exportLibrary.db` — the device name, the browse
//!   categories and sort options, the sub-column, the colour comments and
//!   "Background Color : `OneLibrary`". Read and written by
//!   [`rbl_onelibrary::settings::StickSettings`].
//! - `PIONEER/rekordbox/export.pdb` — copies of the device name and colour
//!   comments, and "Background Color : Device Library" in its `property`
//!   row ([`rbl_pdb::rows::PdbProperty`]).
//!
//! `DEVSETTING.DAT` is 140 bytes and its layout was checked byte for byte
//! against the file on a real rekordbox 7.2.8 export [OBS], and against the
//! rekordcrate and pyrekordbox descriptions of it [REF]:
//!
//! ```text
//!   0x00  u32 LE  0x60          length of the three strings below
//!   0x04  [32]    "PIONEER DJ"  brand, NUL-padded
//!   0x24  [32]    "rekordbox"   software
//!   0x44  [32]    "7.2.11"      version, whatever wrote the file
//!   0x64  u32 LE  32            length of the body
//!   0x68  [32]    body, below
//!   0x88  u16 LE  CRC-16/XMODEM over the 32 body bytes
//!   0x8a  u16     0
//! ```
//!
//! The body: `78 56 34 12 01 00 00 00 01` then, at offsets 9 to 13, the
//! overview waveform type, the waveform colour, a byte that is always `01`,
//! the key display format and the current-position marker, then 18 zeros.
//! The real file read `01 04 01 01 01` there while rekordbox showed Half /
//! 3Band / Classic / CENTER for that stick, which is the check on the enum
//! values. The two `01` bytes whose meaning is not known are carried as read
//! and never changed; one of them may be Waveform Divisions (TIMESCALE on
//! that stick), which is `[UNKNOWN]` until a recording toggles it — so that
//! control is drawn and inert. Body bytes 14, 15 and 16 (file `0x76`–`0x78`)
//! were `01 01 01` after rekordbox 7.2.11 had applied a change on each of
//! its Category, Sort and Color tabs [OBS 2026-09-18 parity run], and `00`
//! from us; the 2026-09-17 run saw only byte 16 vary. [Hypothesis: one flag
//! per customised list.] Carried as read, never set.
//!
//! The CRC was checked against all four setting files on the stick: XMODEM
//! over the body alone reproduces the stored value in `DEVSETTING`,
//! `MYSETTING` and `MYSETTING2`; `DJMMYSETTING` covers the header too [OBS].

use std::path::{Path, PathBuf};

use rbl_onelibrary::settings::StickSettings;

/// "Waveform color": what a player paints the waveform in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveformColor {
    #[default]
    Blue,
    Rgb,
    /// Named "3Band" in rekordbox.
    TriBand,
}

/// "Waveform Current Position": where the play marker sits on a CDJ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveformPosition {
    #[default]
    Center,
    Left,
}

/// "Type of the Overview Waveform".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverviewWaveform {
    #[default]
    Half,
    Full,
}

/// "Key display format".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyDisplay {
    #[default]
    Classic,
    Alphanumeric,
}

/// The settings in `DEVSETTING.DAT`, with every byte not understood kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevSetting {
    pub overview: OverviewWaveform,
    pub color: WaveformColor,
    pub key_display: KeyDisplay,
    pub position: WaveformPosition,
    /// The version string the file carried, written back as read.
    pub version: String,
    /// The whole 32-byte body as read. The four known bytes are patched
    /// into this on write; everything else goes back out untouched.
    body: [u8; BODY_LEN],
}

const STRINGS_LEN: usize = 0x60;
const BODY_LEN: usize = 32;
/// The two lengths as the file spells them.
const STRINGS_LEN_LE: [u8; 4] = [0x60, 0, 0, 0];
const BODY_LEN_LE: [u8; 4] = [0x20, 0, 0, 0];
const FILE_LEN: usize = 4 + STRINGS_LEN + 4 + BODY_LEN + 4;
const BODY_AT: usize = 4 + STRINGS_LEN + 4;
const BRAND: &str = "PIONEER DJ";
const SOFTWARE: &str = "rekordbox";
/// The version the real stick carried [OBS]; what a file we create says.
// The rekordbox this file's layout was last checked against; what
// rekordbox 7.2.11 itself writes here [OBS 2026-09-18 parity run].
const VERSION: &str = "7.2.11";
/// The body of the real stick's file, defaults for a file we create: the
/// two unexplained `01` bytes as observed, and the four known ones at
/// rekordbox's defaults (Half, BLUE, Classic, CENTER) [OBS]/[REF].
const DEFAULT_BODY: [u8; BODY_LEN] = [
    0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const OVERVIEW_AT: usize = 9;
const COLOR_AT: usize = 10;
const KEY_AT: usize = 12;
const POSITION_AT: usize = 13;

impl Default for DevSetting {
    fn default() -> Self {
        Self {
            overview: OverviewWaveform::default(),
            color: WaveformColor::default(),
            key_display: KeyDisplay::default(),
            position: WaveformPosition::default(),
            version: VERSION.to_owned(),
            body: DEFAULT_BODY,
        }
    }
}

/// CRC-16/XMODEM: polynomial 0x1021, no reflection, initial 0.
fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 { crc << 1 } else { (crc << 1) ^ 0x1021 };
        }
    }
    crc
}

fn padded_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl DevSetting {
    /// Parses a file. `None` when it is not a `DEVSETTING.DAT` we understand:
    /// the wrong length, a body length other than 32, or a checksum that
    /// does not match — a file we cannot read faithfully is not one to
    /// write back.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != FILE_LEN {
            return None;
        }
        if bytes.get(0..4)? != STRINGS_LEN_LE
            || bytes.get(4 + STRINGS_LEN..BODY_AT)? != BODY_LEN_LE
        {
            return None;
        }
        let body: [u8; BODY_LEN] = bytes.get(BODY_AT..BODY_AT + BODY_LEN)?.try_into().ok()?;
        let stored = u16::from_le_bytes([bytes[BODY_AT + BODY_LEN], bytes[BODY_AT + BODY_LEN + 1]]);
        if stored != crc16_xmodem(&body) {
            return None;
        }
        let version = padded_string(bytes.get(0x44..0x64)?);
        Some(Self {
            overview: match body[OVERVIEW_AT] {
                0x02 => OverviewWaveform::Full,
                _ => OverviewWaveform::Half,
            },
            color: match body[COLOR_AT] {
                0x03 => WaveformColor::Rgb,
                0x04 => WaveformColor::TriBand,
                _ => WaveformColor::Blue,
            },
            key_display: match body[KEY_AT] {
                0x02 => KeyDisplay::Alphanumeric,
                _ => KeyDisplay::Classic,
            },
            position: match body[POSITION_AT] {
                0x02 => WaveformPosition::Left,
                _ => WaveformPosition::Center,
            },
            version,
            body,
        })
    }

    /// The file, ready to write.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut body = self.body;
        body[OVERVIEW_AT] = match self.overview {
            OverviewWaveform::Half => 0x01,
            OverviewWaveform::Full => 0x02,
        };
        body[COLOR_AT] = match self.color {
            WaveformColor::Blue => 0x01,
            WaveformColor::Rgb => 0x03,
            WaveformColor::TriBand => 0x04,
        };
        body[KEY_AT] = match self.key_display {
            KeyDisplay::Classic => 0x01,
            KeyDisplay::Alphanumeric => 0x02,
        };
        body[POSITION_AT] = match self.position {
            WaveformPosition::Center => 0x01,
            WaveformPosition::Left => 0x02,
        };

        let mut out = Vec::with_capacity(FILE_LEN);
        out.extend_from_slice(&STRINGS_LEN_LE);
        for text in [BRAND, SOFTWARE, self.version.as_str()] {
            let mut field = [0_u8; 32];
            let bytes = text.as_bytes();
            let n = bytes.len().min(31);
            field[..n].copy_from_slice(&bytes[..n]);
            out.extend_from_slice(&field);
        }
        out.extend_from_slice(&BODY_LEN_LE);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc16_xmodem(&body).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }
}

/// Where a stick keeps its export: `PIONEER`, or `.PIONEER` when rekordbox
/// wrote it hidden, which a real stick was found to be [OBS]. Defaults to
/// `PIONEER` on a stick that holds neither.
#[must_use]
pub fn export_root(mount_point: &Path) -> PathBuf {
    for name in ["PIONEER", ".PIONEER"] {
        let root = mount_point.join(name);
        if root.join("rekordbox/export.pdb").is_file() || root.join("rekordbox/exportLibrary.db").is_file() || root.join("DEVSETTING.DAT").is_file() {
            return root;
        }
    }
    mount_point.join("PIONEER")
}

/// Everything the device tabs show for one stick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSettings {
    /// `DEVSETTING.DAT`, or `None` when the stick has none we can read.
    pub dev: Option<DevSetting>,
    /// `exportLibrary.db`'s settings, or `None` when the stick has none.
    pub library: Option<StickSettings>,
    /// "Background Color : Device Library": byte 9 of `export.pdb`'s
    /// `property` row, 0 Default to 8 Purple. `None` when the stick has no
    /// `export.pdb` or the row cannot be read.
    pub device_library_background: Option<u8>,
    /// Whether `export.pdb` is there — "Device Library" in rekordbox's words.
    pub has_device_library: bool,
    /// Whether `exportLibrary.db` is there — "`OneLibrary`".
    pub has_one_library: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("could not update exportLibrary.db: {0}")]
    Library(#[from] rbl_onelibrary::Error),
}

/// Reads what the stick holds. Never fails: a file that is missing or
/// unreadable is simply absent, and the tabs say so.
#[must_use]
pub fn read(mount_point: &Path) -> DeviceSettings {
    if let Err(e) = recover(mount_point) {
        tracing::error!(error = %e, "device recovery failed; settings unavailable");
        return DeviceSettings {
            dev: None,
            library: None,
            device_library_background: None,
            has_device_library: false,
            has_one_library: false,
        };
    }
    read_files(mount_point)
}

fn read_files(mount_point: &Path) -> DeviceSettings {
    let root = export_root(mount_point);
    let dev = std::fs::read(root.join("DEVSETTING.DAT"))
        .ok()
        .and_then(|bytes| DevSetting::parse(&bytes));
    let library_path = root.join("rekordbox/exportLibrary.db");
    let has_one_library = library_path.is_file();
    let library = if has_one_library {
        StickSettings::read(&library_path)
            .map_err(|e| tracing::warn!(path = %library_path.display(), error = %e, "exportLibrary.db unreadable"))
            .ok()
    } else {
        None
    };
    let pdb_path = root.join("rekordbox/export.pdb");
    let has_device_library = pdb_path.is_file();
    let device_library_background = if has_device_library {
        std::fs::read(&pdb_path)
            .ok()
            .and_then(|bytes| rbl_pdb::Pdb::parse(&bytes).ok()?.property())
            .map(|property| property.background_color)
    } else {
        None
    };
    DeviceSettings { dev, library, device_library_background, has_device_library, has_one_library }
}

/// Writes the settings back.
///
/// `DEVSETTING.DAT` is written whole (it is 140 bytes) and created when the
/// stick had none; `exportLibrary.db` is updated in place and only when the
/// stick has one — the tabs cannot invent a library.
pub fn write(mount_point: &Path, settings: &DeviceSettings) -> Result<(), SettingsError> {
    recover(mount_point)?;
    let current = read_files(mount_point);
    write_changes_recovered(mount_point, &current, settings)
}

/// Writes only the settings groups that changed from a previously read stick.
///
/// Display choices live in `DEVSETTING.DAT`; list and colour-label choices
/// live in `exportLibrary.db`. Keeping those writes independent matters when
/// rekordbox has the USB database open: changing the waveform colour must not
/// touch, or be blocked by, an unchanged database.
pub fn write_changes(
    mount_point: &Path,
    current: &DeviceSettings,
    settings: &DeviceSettings,
) -> Result<(), SettingsError> {
    recover(mount_point)?;
    write_changes_recovered(mount_point, current, settings)
}

fn write_changes_recovered(
    mount_point: &Path,
    current: &DeviceSettings,
    settings: &DeviceSettings,
) -> Result<(), SettingsError> {
    let dev_changed = settings.dev.as_ref().is_some_and(|next| current.dev.as_ref() != Some(next));
    let library_changed = settings
        .library
        .as_ref()
        .is_some_and(|next| current.library.as_ref() != Some(next));
    let background_changed = settings.device_library_background.is_some()
        && settings.device_library_background != current.device_library_background;
    if !dev_changed && !library_changed && !background_changed {
        return Ok(());
    }

    let root = export_root(mount_point);
    let publication = rbl_core::durable::Publication::new(mount_point, ".rbxport-publication")?;
    let relative = root.strip_prefix(mount_point).map_err(std::io::Error::other)?;
    let staged_root = publication.stage().join(relative);
    rbl_core::durable::create_dir_all(staged_root.join("rekordbox"))?;
    let mut files = Vec::new();
    if dev_changed {
        if let Some(dev) = &settings.dev {
            rbl_core::durable::write(&staged_root.join("DEVSETTING.DAT"), &dev.encode())?;
            files.push(relative.join("DEVSETTING.DAT"));
        }
    }
    if library_changed {
        if let Some(library) = &settings.library {
            let path = root.join("rekordbox/exportLibrary.db");
            if path.is_file() {
                let staged = staged_root.join("rekordbox/exportLibrary.db");
                std::fs::copy(&path, &staged)?;
                let wal = root.join("rekordbox/exportLibrary.db-wal");
                if wal.exists() { std::fs::copy(wal, staged_root.join("rekordbox/exportLibrary.db-wal"))?; }
                library.write(&staged)?;
                files.extend([relative.join("rekordbox/exportLibrary.db-wal"), relative.join("rekordbox/exportLibrary.db-shm"), relative.join("rekordbox/exportLibrary.db")]);
            }
        }
    }
    if library_changed || background_changed {
        let pdb_path = root.join("rekordbox/export.pdb");
        if let Ok(bytes) = std::fs::read(&pdb_path) {
            let next = updated_pdb(&bytes, library_changed.then_some(settings.library.as_ref()).flatten(), settings.device_library_background);
            if next != bytes {
                rbl_core::durable::write(&staged_root.join("rekordbox/export.pdb"), &next)?;
                files.push(relative.join("rekordbox/export.pdb"));
            }
        }
    }
    publication.commit(&files)?;
    Ok(())
}

/// `export.pdb` with the device panel's changes applied.
///
/// The colour comments and the device name live in both databases.
/// rekordbox renames a colour comment in `export.pdb` too, and a player reads
/// its names from there [OBS 7.2.11]. The `property` row carries the device
/// name beside the Device Library background colour [OBS 7.2.14]. A table
/// that cannot be replaced is left as it was.
fn updated_pdb(bytes: &[u8], library: Option<&StickSettings>, background: Option<u8>) -> Vec<u8> {
    let mut next = bytes.to_vec();
    if let Some(library) = library {
        let rows: Vec<Vec<u8>> = library
            .colors
            .iter()
            .map(|c| rbl_pdb::rows::color_row(u16::try_from(c.id).unwrap_or(0), &c.name))
            .collect();
        if let Some(replaced) = rbl_pdb::build::replace_single_page_table(&next, 6, &rows) {
            next = replaced;
        }
    }
    let property = rbl_pdb::Pdb::parse(&next).ok().and_then(|pdb| pdb.property());
    if let Some(current) = property {
        let mut property = current.clone();
        if let Some(library) = library {
            property.device_name.clone_from(&library.device_name);
        }
        if let Some(background) = background {
            property.background_color = background;
        }
        // An unchanged row keeps rekordbox's page as it was.
        if property == current {
            return next;
        }
        let replaced = rbl_pdb::rows::property_row(&property)
            .and_then(|row| rbl_pdb::build::replace_single_page_table(&next, 19, &[row]));
        if let Some(replaced) = replaced {
            next = replaced;
        }
    }
    next
}

/// Recover all device publication journals before exposing database files.
pub fn recover(mount_point: &Path) -> std::io::Result<()> {
    rbl_export::recover(mount_point)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The real stick's file, byte for byte [OBS].
    const REAL: [u8; 140] = [
        0x60, 0x00, 0x00, 0x00, 0x50, 0x49, 0x4f, 0x4e, 0x45, 0x45, 0x52, 0x20, 0x44, 0x4a, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x72, 0x65, 0x6b, 0x6f, 0x72, 0x64, 0x62, 0x6f, 0x78, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x37, 0x2e, 0x32, 0x2e, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x01, 0x04, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8f, 0x1a, 0x00, 0x00,
    ];

    #[test]
    fn the_crc_is_the_one_the_real_files_carry() {
        // Stored little-endian as 8f 1a.
        assert_eq!(crc16_xmodem(&REAL[BODY_AT..BODY_AT + BODY_LEN]), 0x1a8f);
        // The catalogue check value for CRC-16/XMODEM.
        assert_eq!(crc16_xmodem(b"123456789"), 0x31c3);
    }

    #[test]
    fn the_real_file_reads_as_rekordbox_showed_it() {
        let dev = DevSetting::parse(&REAL).expect("parses");
        assert_eq!(dev.overview, OverviewWaveform::Half);
        assert_eq!(dev.color, WaveformColor::TriBand);
        assert_eq!(dev.key_display, KeyDisplay::Classic);
        assert_eq!(dev.position, WaveformPosition::Center);
        // That stick was written by 7.2.8; a file this writes says 7.2.11.
        assert_eq!(dev.version, "7.2.8");
    }

    #[test]
    fn the_real_file_round_trips_byte_for_byte() {
        let dev = DevSetting::parse(&REAL).unwrap();
        assert_eq!(dev.encode(), REAL.to_vec());
    }

    #[test]
    fn a_change_touches_only_its_own_byte_and_the_checksum() {
        let mut dev = DevSetting::parse(&REAL).unwrap();
        dev.color = WaveformColor::Rgb;
        dev.position = WaveformPosition::Left;
        let out = dev.encode();
        assert_eq!(out.len(), REAL.len());
        let differing: Vec<usize> = (0..REAL.len()).filter(|&i| out[i] != REAL[i]).collect();
        assert_eq!(differing, vec![BODY_AT + COLOR_AT, BODY_AT + POSITION_AT, 0x88, 0x89]);
        assert_eq!(DevSetting::parse(&out).unwrap().encode(), out);
    }

    #[test]
    fn a_fresh_file_is_the_real_one_at_rekordboxs_defaults() {
        let out = DevSetting::default().encode();
        let parsed = DevSetting::parse(&out).expect("our own file parses");
        assert_eq!(parsed, DevSetting::default());
        // Same everywhere except the one setting that stick had changed and
        // the version, "7.2.8" there and "7.2.11" here (bytes 0x48, 0x49:
        // "8" against "11").
        let differing: Vec<usize> = (0..REAL.len()).filter(|&i| out[i] != REAL[i]).collect();
        assert_eq!(differing, vec![0x48, 0x49, BODY_AT + COLOR_AT, 0x88, 0x89]);
    }

    #[test]
    fn a_damaged_file_is_refused_rather_than_rewritten() {
        let mut bad = REAL;
        bad[BODY_AT + COLOR_AT] = 0x03; // without fixing the checksum
        assert!(DevSetting::parse(&bad).is_none());
        assert!(DevSetting::parse(&REAL[..100]).is_none());
        let mut wrong_len = REAL;
        wrong_len[4 + STRINGS_LEN] = 40;
        assert!(DevSetting::parse(&wrong_len).is_none());
    }

    #[test]
    fn a_stick_with_nothing_reads_as_nothing_and_a_written_file_reads_back() {
        let stick = tempfile::tempdir().unwrap();
        let empty = read(stick.path());
        assert_eq!(empty.dev, None);
        assert_eq!(empty.library, None);
        assert!(!empty.has_device_library);
        assert!(!empty.has_one_library);

        let dev = DevSetting { key_display: KeyDisplay::Alphanumeric, ..DevSetting::default() };
        write(stick.path(), &DeviceSettings { dev: Some(dev.clone()), ..empty }).unwrap();
        let back = read(stick.path()).dev.expect("a file now");
        assert_eq!(back.key_display, KeyDisplay::Alphanumeric);
        assert_eq!(back.encode(), dev.encode());
        assert!(stick.path().join("PIONEER/DEVSETTING.DAT").is_file());
    }

    #[test]
    fn a_display_change_does_not_touch_unchanged_library_settings() {
        let stick = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(stick.path().join("PIONEER")).unwrap();
        let current = DeviceSettings {
            dev: Some(DevSetting::default()),
            library: Some(StickSettings::default()),
            device_library_background: None,
            has_device_library: true,
            has_one_library: true,
        };
        std::fs::write(
            stick.path().join("PIONEER/DEVSETTING.DAT"),
            current.dev.as_ref().unwrap().encode(),
        )
        .unwrap();
        let mut next = current.clone();
        next.dev.as_mut().unwrap().color = WaveformColor::Rgb;

        // There is deliberately no exportLibrary.db fixture. The unchanged
        // library half must not be opened merely to write DEVSETTING.DAT.
        write_changes(stick.path(), &current, &next).unwrap();

        let written = read(stick.path()).dev.expect("the display file remains readable");
        assert_eq!(written.color, WaveformColor::Rgb);
        assert!(!stick.path().join("PIONEER/rekordbox/exportLibrary.db").exists());
    }

    #[test]
    fn both_background_colours_are_written_where_rekordbox_keeps_them() {
        let stick = tempfile::tempdir().unwrap();
        assert!(rbl_export::create_library(stick.path(), None, &[], None).unwrap());
        let current = read(stick.path());
        assert_eq!(current.device_library_background, Some(0));
        assert_eq!(current.library.as_ref().unwrap().background_color_type, 0);

        // OneLibrary Purple, Device Library Yellow, and a new name.
        let mut next = current.clone();
        let library = next.library.as_mut().unwrap();
        library.background_color_type = 8;
        library.device_name = "FRIDAY".to_owned();
        next.device_library_background = Some(4);
        write_changes(stick.path(), &current, &next).unwrap();

        let back = read(stick.path());
        assert_eq!(back.library.as_ref().unwrap().background_color_type, 8);
        assert_eq!(back.device_library_background, Some(4));
        let bytes = std::fs::read(stick.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
        let property = rbl_pdb::Pdb::parse(&bytes).unwrap().property().unwrap();
        assert_eq!(property.device_name, "FRIDAY");
        assert_eq!(property.background_color, 4);

        // Only the Device Library colour: exportLibrary.db is not rewritten.
        let library_path = stick.path().join("PIONEER/rekordbox/exportLibrary.db");
        let library_before = std::fs::read(&library_path).unwrap();
        let mut blue = back.clone();
        blue.device_library_background = Some(7);
        write_changes(stick.path(), &back, &blue).unwrap();
        assert_eq!(read(stick.path()).device_library_background, Some(7));
        assert_eq!(std::fs::read(&library_path).unwrap(), library_before);
    }

    #[test]
    fn a_library_change_leaves_an_unchanged_property_page_alone() {
        let current = StickSettings::default();
        let property = rbl_pdb::rows::PdbProperty { created_date: "2026-05-08".to_owned(), ..Default::default() };
        let mut file = rbl_pdb::build::FileBuilder::new(4096);
        file.add_table(19, &[rbl_pdb::rows::property_row(&property).unwrap()]);
        let bytes = file.finish();
        assert_eq!(updated_pdb(&bytes, Some(&current), None), bytes);
        assert_eq!(updated_pdb(&bytes, None, Some(0)), bytes);
        assert_ne!(updated_pdb(&bytes, None, Some(5)), bytes);
    }

    #[test]
    fn a_hidden_export_root_is_found() {
        let stick = tempfile::tempdir().unwrap();
        assert_eq!(export_root(stick.path()), stick.path().join("PIONEER"));
        std::fs::create_dir_all(stick.path().join(".PIONEER/rekordbox")).unwrap();
        std::fs::write(stick.path().join(".PIONEER/rekordbox/export.pdb"), b"x").unwrap();
        assert_eq!(export_root(stick.path()), stick.path().join(".PIONEER"));
    }
}
