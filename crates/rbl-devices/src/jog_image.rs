//! Explicit USB jog-image edits. Image and display mode are independent actions.
use std::{
    io::{self, Read},
    path::{Component, Path},
};

use crate::settings::{self, DevSetting, JogImage};
use rbl_core::durable::{Publication, VolumeIdentity};

/// Matches the existing artwork-source admission bound used by LINK.
pub const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
/// An 800-square exported JPEG fits the existing LINK artwork bound.
const MAX_JPEG_BYTES: u64 = 4 * 1024 * 1024;
pub const FILE_NAME: &str = "rekordbox_customimage.jpeg";

/// Validate and convert a chosen JPEG/PNG to the current writer's square,
/// black-matted JPEG, without upscaling images smaller than 800 pixels.
pub fn prepare(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let format = image::guess_format(bytes).map_err(io::Error::other)?;
    if !matches!(format, image::ImageFormat::Jpeg | image::ImageFormat::Png) {
        return Err(io::Error::other("Choose a JPEG or PNG image."));
    }
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(io::Error::other(
            "The chosen image is too large (maximum 32 MiB).",
        ));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    // The existing export artwork decoder uses these limits as well.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(io::Error::other)?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    let side = width.max(height).min(800);
    if side == 0 {
        return Err(io::Error::other("The image is empty."));
    }
    let mut rgb = image::RgbImage::new(width, height);
    for (pixel, source) in rgb.pixels_mut().zip(rgba.pixels()) {
        for channel in 0..3 {
            pixel[channel] =
                u8::try_from((u16::from(source[channel]) * u16::from(source[3]) + 127) / 255)
                    .map_err(io::Error::other)?;
        }
    }
    drop(rgba);
    let original_side = width.max(height);
    let square = if width == height {
        rgb
    } else {
        let mut square = image::RgbImage::new(original_side, original_side);
        image::imageops::replace(
            &mut square,
            &rgb,
            i64::from((original_side - width) / 2),
            i64::from((original_side - height) / 2),
        );
        square
    };
    let square = if original_side == side {
        square
    } else {
        image::imageops::resize(&square, side, side, image::imageops::FilterType::Lanczos3)
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&square)
        .map_err(io::Error::other)?;
    Ok(out)
}

fn plain_path(root: &Path, relative: &Path) -> io::Result<()> {
    let mut path = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(io::Error::other("Invalid image path."));
        };
        path.push(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::other(
                    "The device image or settings path is a symbolic link.",
                ));
            }
            Ok(meta) if index + 1 == components.len() && !meta.is_file() => {
                return Err(io::Error::other(
                    "The image or settings path is not a regular file.",
                ));
            }
            Ok(meta) if index + 1 != components.len() && !meta.is_dir() => {
                return Err(io::Error::other(
                    "The image or settings parent is not a directory.",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Returns a bounded, validated JPEG for preview/copy. The stored file is never modified.
pub fn exists(root: &Path) -> io::Result<bool> {
    plain_path(root, Path::new(FILE_NAME))?;
    match std::fs::symlink_metadata(root.join(FILE_NAME)) {
        Ok(meta) => Ok(meta.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn read(root: &Path) -> io::Result<Option<Vec<u8>>> {
    plain_path(root, Path::new(FILE_NAME))?;
    let file = match std::fs::File::open(root.join(FILE_NAME)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("The image path is not a file."));
    }
    let mut bytes = Vec::new();
    file.take(MAX_JPEG_BYTES + 1).read_to_end(&mut bytes)?;
    validate(&bytes)?;
    Ok(Some(bytes))
}

pub fn validate(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() as u64 > MAX_JPEG_BYTES {
        return Err(io::Error::other(
            "The stored image is too large (maximum 4 MiB).",
        ));
    }
    if image::guess_format(bytes).map_err(io::Error::other)? != image::ImageFormat::Jpeg {
        return Err(io::Error::other("The stored image is not a JPEG."));
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(800);
    limits.max_image_height = Some(800);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(io::Error::other)?;
    Ok(())
}

fn dev_relative(root: &Path) -> io::Result<std::path::PathBuf> {
    let relative = settings::export_root(root)
        .strip_prefix(root)
        .map_err(io::Error::other)?
        .join("DEVSETTING.DAT");
    plain_path(root, &relative)?;
    Ok(relative)
}

fn read_dev_bytes(root: &Path, relative: &Path) -> io::Result<Option<Vec<u8>>> {
    plain_path(root, relative)?;
    let file = match std::fs::File::open(root.join(relative)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(141).read_to_end(&mut bytes)?;
    if DevSetting::parse(&bytes).is_none() {
        return Err(io::Error::other(
            "The device settings are unreadable; they were left unchanged.",
        ));
    }
    Ok(Some(bytes))
}

pub fn mode(root: &Path) -> io::Result<Option<JogImage>> {
    Ok(read_dev_bytes(root, &dev_relative(root)?)?
        .as_deref()
        .and_then(DevSetting::parse)
        .and_then(|settings| settings.jog_image()))
}

#[derive(Clone, Copy)]
pub enum Change<'a> {
    Mode(JogImage),
    Image(&'a [u8]),
    Delete,
}

/// `identity` is captured before chooser/source preparation, then checked again
/// after acquiring the same publication lock used by exports and settings.
pub fn write(root: &Path, identity: &VolumeIdentity, change: Change<'_>) -> io::Result<()> {
    identity.check(root)?;
    let publication = Publication::new_checked(root, ".rbxport-publication", identity)?;
    identity.check(root)?;
    let (relative, bytes) = match change {
        Change::Mode(mode) => {
            let relative = dev_relative(root)?;
            let before =
                read_dev_bytes(root, &relative)?.unwrap_or_else(|| DevSetting::default().encode());
            let bytes = DevSetting::patch_jog_image(&before, mode).ok_or_else(|| {
                io::Error::other("The device settings are unreadable; they were left unchanged.")
            })?;
            if bytes == before && root.join(&relative).is_file() {
                return Ok(());
            }
            (relative, Some(bytes))
        }
        Change::Image(bytes) => {
            plain_path(root, Path::new(FILE_NAME))?;
            validate(bytes)?;
            if read(root).ok().flatten().as_deref() == Some(bytes) {
                return Ok(());
            }
            (Path::new(FILE_NAME).to_path_buf(), Some(bytes.to_vec()))
        }
        Change::Delete => {
            plain_path(root, Path::new(FILE_NAME))?;
            match std::fs::symlink_metadata(root.join(FILE_NAME)) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => return Err(io::Error::other("The image path is not a file.")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            }
            (Path::new(FILE_NAME).to_path_buf(), None)
        }
    };
    if let Some(bytes) = bytes {
        let staged = publication.stage().join(&relative);
        if let Some(parent) = staged.parent() {
            rbl_core::durable::create_dir_all(parent)?;
        }
        rbl_core::durable::write(&staged, &bytes)?;
    }
    publication.commit(&[relative])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(color)));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    #[test]
    fn sizes_padding_and_alpha_match_the_jog_image_contract() {
        for (width, height, expected) in [
            (799, 799, 799),
            (800, 800, 800),
            (801, 801, 800),
            (640, 320, 640),
            (320, 640, 640),
            (1200, 600, 800),
            (600, 1200, 800),
        ] {
            let bytes = prepare(&png(width, height, [240, 80, 20, 255])).unwrap();
            assert_eq!(&bytes[..2], &[0xff, 0xd8]);
            let rgb = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert_eq!(rgb.dimensions(), (expected, expected));
            assert!(rgb.get_pixel(expected / 2, expected / 2)[0] > 220);
            if width != height {
                let corner = rgb.get_pixel(0, 0);
                assert!(
                    corner.0.iter().all(|&channel| channel < 5),
                    "{width}x{height}: {corner:?}"
                );
            }
        }
        for (alpha, red) in [(0, 0), (128, 120), (255, 240)] {
            let bytes = prepare(&png(32, 32, [240, 80, 20, alpha])).unwrap();
            let rgb = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert!((i16::from(rgb.get_pixel(16, 16)[0]) - red).abs() < 5);
        }
        assert!(prepare(b"not an image").is_err());
    }
    #[test]
    fn explicit_image_actions_preserve_mode_and_other_files() {
        for name in ["PIONEER", ".PIONEER"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join(name)).unwrap();
            let settings_path = root.path().join(name).join("DEVSETTING.DAT");
            let before = DevSetting::default().encode();
            std::fs::write(&settings_path, &before).unwrap();
            std::fs::write(root.path().join("unmanaged.txt"), b"keep").unwrap();
            let identity = VolumeIdentity::capture(root.path()).unwrap();
            let jpeg = prepare(&png(64, 32, [255, 100, 0, 255])).unwrap();
            write(root.path(), &identity, Change::Image(&jpeg)).unwrap();
            assert_eq!(std::fs::read(&settings_path).unwrap(), before);
            write(root.path(), &identity, Change::Mode(JogImage::Custom)).unwrap();
            assert_eq!(read(root.path()).unwrap(), Some(jpeg.clone()));
            assert_eq!(std::fs::read(&settings_path).unwrap()[0x77], 2);
            write(root.path(), &identity, Change::Image(&jpeg)).unwrap();
            write(root.path(), &identity, Change::Delete).unwrap();
            assert!(read(root.path()).unwrap().is_none());
            assert_eq!(std::fs::read(&settings_path).unwrap()[0x77], 2);
            assert_eq!(
                std::fs::read(root.path().join("unmanaged.txt")).unwrap(),
                b"keep"
            );
            std::fs::write(&settings_path, b"bad settings").unwrap();
            assert!(write(root.path(), &identity, Change::Mode(JogImage::Artwork)).is_err());
            assert_eq!(std::fs::read(&settings_path).unwrap(), b"bad settings");
        }
    }
    #[test]
    fn pending_export_and_replaced_root_refuse_before_publication() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("device");
        std::fs::create_dir(&root).unwrap();
        let identity = VolumeIdentity::capture(&root).unwrap();
        let pending = root.join(".rbxport-publication");
        std::fs::create_dir(&pending).unwrap();
        std::fs::write(pending.join("evidence"), b"pending").unwrap();
        assert!(write(&root, &identity, Change::Mode(JogImage::Artwork)).is_err());
        assert_eq!(std::fs::read(pending.join("evidence")).unwrap(), b"pending");
        assert!(!root.join("PIONEER").exists());
        std::fs::rename(&root, parent.path().join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(write(&root, &identity, Change::Mode(JogImage::Artwork)).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
    #[test]
    fn malformed_and_oversized_images_can_be_replaced_or_deleted_without_touching_settings() {
        let root = tempfile::tempdir().unwrap();
        let identity = VolumeIdentity::capture(root.path()).unwrap();
        let path = root.path().join(FILE_NAME);
        let jpeg = prepare(&png(32, 16, [255, 0, 0, 255])).unwrap();
        for oversized in [false, true] {
            std::fs::write(&path, b"invalid").unwrap();
            if oversized {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len(MAX_JPEG_BYTES + 1)
                    .unwrap();
            }
            assert!(read(root.path()).is_err());
            write(root.path(), &identity, Change::Image(&jpeg)).unwrap();
            assert_eq!(read(root.path()).unwrap(), Some(jpeg.clone()));
            std::fs::write(&path, b"invalid").unwrap();
            write(root.path(), &identity, Change::Delete).unwrap();
            assert!(!path.exists());
        }
        assert!(!root.path().join("PIONEER").exists());
        assert!(prepare(&png(4097, 1, [255, 0, 0, 255])).is_err());
    }

    #[test]
    fn new_mode_reads_and_writes_refuse_oversized_settings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("PIONEER")).unwrap();
        let path = root.path().join("PIONEER/DEVSETTING.DAT");
        std::fs::write(&path, DevSetting::default().encode()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(8 * 1024 * 1024)
            .unwrap();
        let before = std::fs::metadata(&path).unwrap();
        assert!(mode(root.path()).is_err());
        let identity = VolumeIdentity::capture(root.path()).unwrap();
        assert!(write(root.path(), &identity, Change::Mode(JogImage::Custom)).is_err());
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    }
    #[test]
    fn explicit_edits_after_export_survive_metadata_resync_and_noop() {
        use rbl_export::{ExportOptions, ExportRoot, SourcePlaylist, SourceTrack, SyncSource};
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("generated.wav");
        let samples = [0_u8; 64];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&100_u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44100_u32.to_le_bytes());
        wav.extend_from_slice(&88200_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&64_u32.to_le_bytes());
        wav.extend_from_slice(&samples);
        std::fs::write(&source, &wav).unwrap();
        let jpeg = prepare(&png(64, 32, [255, 64, 0, 255])).unwrap();
        for (name, export_root) in [
            ("PIONEER", ExportRoot::Standard),
            (".PIONEER", ExportRoot::Hidden),
        ] {
            let root = temporary.path().join(
                name.trim_start_matches('.').to_owned()
                    + if name.starts_with('.') {
                        "-hidden"
                    } else {
                        "-standard"
                    },
            );
            let mut tracks = [SourceTrack {
                id: 1,
                source_path: source.clone(),
                title: "Before".into(),
                artist: "Generated".into(),
                sample_rate: 44100,
                analysis: vec![(
                    "DAT".into(),
                    rbl_anlz::AnlzBuilder::new().path("/generated.wav").finish(),
                )],
                ..Default::default()
            }];
            let playlists = [SourcePlaylist {
                id: 1,
                name: "Generated".into(),
                track_indices: vec![0],
                ..Default::default()
            }];
            let sync = SyncSource {
                db_id: 714,
                ..Default::default()
            };
            let options = ExportOptions {
                root: Some(export_root),
                sync: Some(&sync),
                ..Default::default()
            };
            assert!(
                rbl_export::export_with_options(
                    &root,
                    &tracks,
                    &playlists,
                    &[],
                    &options,
                    &mut |_| {}
                )
                .unwrap()
                .verified
            );
            let identity = VolumeIdentity::capture(&root).unwrap();
            write(&root, &identity, Change::Image(&jpeg)).unwrap();
            write(&root, &identity, Change::Mode(JogImage::Custom)).unwrap();
            let settings = std::fs::read(root.join(name).join("DEVSETTING.DAT")).unwrap();
            tracks[0].title = "After".into();
            let changed = rbl_export::export_with_options(
                &root,
                &tracks,
                &playlists,
                &[],
                &options,
                &mut |_| {},
            )
            .unwrap();
            assert!(changed.verified);
            assert_eq!(changed.bytes_copied, 0);
            let snapshot = rbl_export::snapshot::Snapshot::read(&root).unwrap();
            assert_eq!(snapshot.legacy.unwrap().tracks[0].title, "After");
            assert_eq!(snapshot.one.unwrap().tracks[0].title, "After");
            assert_eq!(read(&root).unwrap(), Some(jpeg.clone()));
            assert_eq!(
                std::fs::read(root.join(name).join("DEVSETTING.DAT")).unwrap(),
                settings
            );
            let repeat = rbl_export::export_with_options(
                &root,
                &tracks,
                &playlists,
                &[],
                &options,
                &mut |_| {},
            )
            .unwrap();
            assert!(repeat.verified && repeat.up_to_date);
            assert_eq!(repeat.bytes_copied, 0);
            assert_eq!(std::fs::read(&source).unwrap(), wav);
        }
    }
}
