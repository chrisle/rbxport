//! Safe OS ejection after all export handles have closed. Never force a busy volume.
//!
//! On Windows this is the request "Safely Remove Hardware" makes
//! (`CM_Request_Device_Eject`), which needs no administrator rights. Opening
//! the volume for read and write to lock it does need them, so a normal
//! (not elevated) app got "Access is denied (os error 5)" that way.
use std::{io, path::Path};

pub fn eject(path: &Path) -> io::Result<()> {
    // Enumerate real volumes: test directories and arbitrary paths must never
    // turn into an eject request for the disk containing them.
    let disks = sysinfo::Disks::new_with_refreshed_list();
    if !super::devices_from(&disks)
        .iter()
        .any(|device| device.mount_point == path)
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "The USB volume is no longer connected.",
        ));
    }
    platform_eject(path)
}

#[cfg(not(windows))]
fn run(command: &mut std::process::Command) -> io::Result<std::process::Output> {
    let output = command.output()?;
    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(io::Error::other(if detail.is_empty() {
            "The system could not eject the device. It may be in use.".to_owned()
        } else {
            detail
        }))
    }
}

#[cfg(target_os = "macos")]
fn platform_eject(path: &Path) -> io::Result<()> {
    run(std::process::Command::new("/usr/sbin/diskutil")
        .arg("eject")
        .arg(path))
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn platform_eject(path: &Path) -> io::Result<()> {
    let source = run(std::process::Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "--mountpoint"])
        .arg(path))?;
    let source = String::from_utf8_lossy(&source.stdout);
    let source = source.trim();
    if !source.starts_with("/dev/") || source.lines().count() != 1 {
        return Err(io::Error::other("Could not identify the USB volume."));
    }
    run(std::process::Command::new("udisksctl").args([
        "unmount",
        "--no-user-interaction",
        "--block-device",
        source,
    ]))
    .map(|_| ())
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::platform_eject;

/// The drive letter of a Windows drive root such as `E:\`, or `None` for any
/// other path. A volume mounted in a folder has no letter and is refused.
#[cfg_attr(not(windows), allow(dead_code))]
fn drive_letter(path: &Path) -> Option<char> {
    let text = path.as_os_str().to_string_lossy();
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next(), chars.next()) {
        (Some(letter), Some(':'), Some('\\' | '/'), None) if letter.is_ascii_alphabetic() => {
            Some(letter.to_ascii_uppercase())
        }
        _ => None,
    }
}

/// The strings in a Windows multi-string (`REG_MULTI_SZ`) buffer: each one
/// ends in a NUL and an empty string ends the list.
#[cfg_attr(not(windows), allow(dead_code))]
fn multi_strings(buffer: &[u16]) -> impl Iterator<Item = &[u16]> {
    buffer.split(|&unit| unit == 0).take_while(|item| !item.is_empty())
}

/// Why Windows refused to let the stick go, as `PNP_VETO_TYPE` reports it
/// (cfg.h), with the process, service or device it names.
#[cfg_attr(not(windows), allow(dead_code))]
fn veto_message(veto_type: i32, name: &str) -> String {
    // PNP_VetoWindowsApp names the program; the other open-handle vetoes
    // name a device or driver, which means nothing to the user.
    const WINDOWS_APP: i32 = 3;
    const WINDOWS_SERVICE: i32 = 4;
    const OUTSTANDING_OPEN: i32 = 5;
    const PENDING_CLOSE: i32 = 2;
    const INSUFFICIENT_RIGHTS: i32 = 12;
    const ALREADY_REMOVED: i32 = 13;
    // A program arrives as an NT path such as
    // `\Device\HarddiskVolume3\Windows\explorer.exe`; its file name is enough.
    let name = name.trim().rsplit(['\\', '/']).next().unwrap_or_default().trim();
    match veto_type {
        WINDOWS_APP | WINDOWS_SERVICE if !name.is_empty() => format!(
            "The USB is in use by {name}. Close it, then eject the USB again."
        ),
        WINDOWS_APP | WINDOWS_SERVICE | OUTSTANDING_OPEN | PENDING_CLOSE => {
            "The USB is in use by another program. Close any window or program using it, then eject the USB again.".to_owned()
        }
        INSUFFICIENT_RIGHTS => "Windows does not allow this account to eject the USB.".to_owned(),
        ALREADY_REMOVED => "The USB is no longer connected.".to_owned(),
        _ => "Windows could not eject the USB. It may be in use.".to_owned(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_eject(_: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Eject is not supported on this platform.",
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_directory_is_never_treated_as_a_volume_to_eject() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(super::eject(directory.path()).unwrap_err().kind(), std::io::ErrorKind::NotFound);
        assert!(directory.path().is_dir());
    }

    #[test]
    fn only_a_drive_root_has_a_letter_to_eject() {
        use std::path::Path;
        assert_eq!(super::drive_letter(Path::new("E:\\")), Some('E'));
        assert_eq!(super::drive_letter(Path::new("f:/")), Some('F'));
        assert_eq!(super::drive_letter(Path::new("E:")), None);
        assert_eq!(super::drive_letter(Path::new("E:\\Music")), None);
        assert_eq!(super::drive_letter(Path::new("\\\\.\\E:")), None);
        assert_eq!(super::drive_letter(Path::new("/Volumes/USB")), None);
        assert_eq!(super::drive_letter(Path::new("1:\\")), None);
    }

    #[test]
    fn a_multi_string_lists_each_interface_once() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let mut buffer = wide("\\\\?\\usbstor#disk#a");
        buffer.push(0);
        buffer.extend(wide("\\\\?\\scsi#disk#b"));
        buffer.extend([0, 0, 0]);
        let items: Vec<String> = super::multi_strings(&buffer).map(String::from_utf16_lossy).collect();
        assert_eq!(items, ["\\\\?\\usbstor#disk#a", "\\\\?\\scsi#disk#b"]);
        assert_eq!(super::multi_strings(&[0, 0]).count(), 0);
        assert_eq!(super::multi_strings(&[]).count(), 0);
    }

    #[test]
    fn a_veto_says_what_holds_the_usb_without_a_raw_os_error() {
        let app = super::veto_message(3, "\\Device\\HarddiskVolume7\\Windows\\explorer.exe");
        assert!(app.contains("explorer.exe"), "{app}");
        assert!(super::veto_message(5, "USBSTOR\\Disk").contains("another program"));
        assert!(super::veto_message(3, "").contains("another program"));
        assert!(super::veto_message(13, "").contains("no longer connected"));
        for veto in 0..16 {
            assert!(!super::veto_message(veto, "x").contains("os error"));
        }
    }
}
