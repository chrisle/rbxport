//! Safe OS ejection after all export handles have closed. Never force a busy volume.
//!
//! On Windows this is the request "Safely Remove Hardware" makes
//! (`CM_Request_Device_Eject`), which asks every program holding a file on
//! the stick to let go first. Locking the volume (`FSCTL_LOCK_VOLUME`)
//! instead fails with "Access is denied (os error 5)" while any program,
//! even Explorer or the search indexer, has a file open on it, elevated or
//! not. The lock is only used for removable media with no device Windows
//! offers for safe removal, such as a card in a built-in SD reader.
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

/// How a Windows eject is carried out.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
enum EjectTarget<N> {
    /// Ask Plug and Play to remove this device node, as Safely Remove does.
    Device(N),
    /// No device is removable but the media is (a card in a built-in SD
    /// reader): lock, dismount and eject the media, as Explorer's Eject does.
    Media,
    /// Neither, such as a disk inside the computer.
    Refuse,
}

/// What the eject choice needs to know about one Plug and Play device node.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DeviceNode {
    /// `CM_DEVCAP_REMOVABLE`: removable from its immediate parent, which is
    /// what puts a device in "Safely Remove Hardware".
    removable: bool,
    /// A USB interface of a composite device (`USB\VID_…&PID_…&MI_nn\…`).
    interface: bool,
    /// A USB hub, root or external. Ejecting one takes down every device
    /// plugged into it (a dock's keyboard, network, audio interface).
    hub: bool,
}

/// Whether a device instance id names a USB interface node
/// (`USB\VID_05AC&PID_8600&MI_00\6&…` on chris-win11).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_interface_id(instance_id: &str) -> bool {
    let mut parts = instance_id.split('\\');
    parts.next().is_some_and(|bus| bus.eq_ignore_ascii_case("USB"))
        && parts
            .next()
            .is_some_and(|id| id.to_ascii_uppercase().contains("&MI_"))
}

/// Whether a device is a USB hub: a root hub (`USB\ROOT_HUB…`), a device
/// with the hub class among its compatible ids (`USB\Class_09…`), or one run
/// by a hub driver (`usbhub`, `USBHUB3`).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_hub<'a>(
    instance_id: &str,
    mut compatible_ids: impl Iterator<Item = &'a str>,
    service: &str,
) -> bool {
    let starts = |text: &str, prefix: &str| {
        text.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    starts(instance_id, "USB\\ROOT_HUB")
        || compatible_ids.any(|id| starts(id, "USB\\Class_09"))
        || service.eq_ignore_ascii_case("usbhub")
        || service.eq_ignore_ascii_case("usbhub3")
}

/// Chooses what to eject for `disk`, the device Safely Remove would offer.
/// `CM_DEVCAP_REMOVABLE` means removable from the immediate parent, so that
/// is the disk's parent for a stick. On a composite device (many card
/// readers) the parent is an interface node and the removable device is the
/// one above it, so the walk goes up a second level only through an
/// interface node. Anything further, or a non-interface node in between,
/// reaches hubs and controllers: an external hub (a dock's SD slot sits
/// under one) and a Thunderbolt USB controller can both be removable. A hub
/// is never chosen. Failing that, the disk itself; then the media when the
/// disk reports removable media, as Explorer's Eject does.
#[cfg_attr(not(windows), allow(dead_code))]
fn eject_target<N: Copy>(
    disk: N,
    removable_media: bool,
    mut parent: impl FnMut(N) -> Option<N>,
    mut describe: impl FnMut(N) -> DeviceNode,
) -> EjectTarget<N> {
    let ejectable = |node: DeviceNode| node.removable && !node.hub;
    if let Some(up) = parent(disk) {
        let first = describe(up);
        if ejectable(first) {
            return EjectTarget::Device(up);
        }
        if first.interface && !first.hub {
            if let Some(device) = parent(up) {
                if ejectable(describe(device)) {
                    return EjectTarget::Device(device);
                }
            }
        }
    }
    if ejectable(describe(disk)) {
        EjectTarget::Device(disk)
    } else if removable_media {
        EjectTarget::Media
    } else {
        EjectTarget::Refuse
    }
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

    /// A device tree as (node, parent, kind) rows: kind is the node's
    /// `DeviceNode`.
    fn target(tree: &[(u32, Option<u32>, super::DeviceNode)], disk: u32, removable_media: bool) -> super::EjectTarget<u32> {
        let row = |node: u32| tree.iter().find(|row| row.0 == node).copied();
        super::eject_target(
            disk,
            removable_media,
            |node| row(node).and_then(|row| row.1),
            |node| row(node).map(|row| row.2).unwrap_or_default(),
        )
    }

    const FIXED: super::DeviceNode = super::DeviceNode { removable: false, interface: false, hub: false };
    const REMOVABLE: super::DeviceNode = super::DeviceNode { removable: true, interface: false, hub: false };
    const INTERFACE: super::DeviceNode = super::DeviceNode { removable: false, interface: true, hub: false };
    const HUB: super::DeviceNode = super::DeviceNode { removable: false, interface: false, hub: true };
    const REMOVABLE_HUB: super::DeviceNode = super::DeviceNode { removable: true, interface: false, hub: true };

    #[test]
    fn a_stick_ejects_its_usb_device() {
        use super::EjectTarget::Device;
        // Disk -> USB device (removable) -> root hub, as on chris-win11.
        let tree = [(1, Some(2), FIXED), (2, Some(3), REMOVABLE), (3, Some(4), HUB), (4, None, REMOVABLE)];
        assert_eq!(target(&tree, 1, false), Device(2));
        assert_eq!(target(&tree, 1, true), Device(2));
    }

    #[test]
    fn a_composite_reader_ejects_the_device_above_its_interface() {
        use super::EjectTarget::Device;
        // Disk -> USB\VID_…&PID_…&MI_00 interface (not removable) -> composite device (removable).
        let tree = [(1, Some(2), FIXED), (2, Some(3), INTERFACE), (3, Some(4), REMOVABLE), (4, None, HUB)];
        assert_eq!(target(&tree, 1, false), Device(3));
        assert_eq!(target(&tree, 1, true), Device(3));
    }

    #[test]
    fn a_reader_in_a_dock_never_ejects_the_dock_hub() {
        use super::EjectTarget::{Media, Refuse};
        // Dock SD slot: disk -> reader USB device (not removable) -> external hub (removable).
        let tree = [(1, Some(2), FIXED), (2, Some(3), FIXED), (3, Some(4), REMOVABLE_HUB), (4, None, HUB)];
        assert_eq!(target(&tree, 1, true), Media);
        assert_eq!(target(&tree, 1, false), Refuse);
        // The same with the hub not flagged as one: still not reached, since
        // the level in between is not an interface node.
        let tree = [(1, Some(2), FIXED), (2, Some(3), FIXED), (3, Some(4), REMOVABLE), (4, None, HUB)];
        assert_eq!(target(&tree, 1, true), Media);
        assert_eq!(target(&tree, 1, false), Refuse);
        // A removable hub is never chosen, even above an interface node or
        // directly above the disk.
        let tree = [(1, Some(2), FIXED), (2, Some(3), INTERFACE), (3, None, REMOVABLE_HUB)];
        assert_eq!(target(&tree, 1, true), Media);
        let tree = [(1, Some(2), FIXED), (2, None, REMOVABLE_HUB)];
        assert_eq!(target(&tree, 1, false), Refuse);
    }

    #[test]
    fn the_walk_never_reaches_a_removable_controller() {
        use super::EjectTarget::{Device, Media, Refuse};
        // Disk -> device -> root hub -> Thunderbolt xHCI (removable, cap 0x84 on chris-win11).
        let tree = [(1, Some(2), FIXED), (2, Some(3), FIXED), (3, Some(4), HUB), (4, None, REMOVABLE)];
        assert_eq!(target(&tree, 1, false), Refuse);
        assert_eq!(target(&tree, 1, true), Media);
        // Through an interface node the walk still stops at the device above it.
        let tree = [(1, Some(2), FIXED), (2, Some(3), INTERFACE), (3, Some(4), FIXED), (4, None, REMOVABLE)];
        assert_eq!(target(&tree, 1, true), Media);
        // A removable disk under a fixed parent goes itself.
        let tree = [(1, Some(2), REMOVABLE), (2, Some(3), FIXED), (3, None, FIXED)];
        assert_eq!(target(&tree, 1, false), Device(1));
    }

    #[test]
    fn an_internal_disk_is_refused_and_a_built_in_card_reader_ejects_the_media() {
        use super::EjectTarget::{Media, Refuse};
        // Disk -> NVMe/SD host controller -> root port, none removable.
        let tree = [(1, Some(2), FIXED), (2, Some(3), FIXED), (3, None, FIXED)];
        assert_eq!(target(&tree, 1, false), Refuse);
        assert_eq!(target(&tree, 1, true), Media);
        // A disk with no parent at all.
        assert_eq!(target(&[(1, None, FIXED)], 1, false), Refuse);
    }

    #[test]
    fn interface_and_hub_nodes_are_told_apart_by_their_ids() {
        // Instance ids as listed on chris-win11 (values-windows-usb-nodes.txt).
        assert!(super::is_interface_id("USB\\VID_05AC&PID_8600&MI_00\\6&C3BB1E1&0&0000"));
        assert!(super::is_interface_id("usb\\vid_05ac&pid_8600&mi_03\\6&c3bb1e1&0&0003"));
        assert!(!super::is_interface_id("USB\\VID_05AC&PID_8600\\5&1F3E3F24&0&3"));
        assert!(!super::is_interface_id("USB\\VID_0781&PID_55BB\\MSFT30323233363654343031313833"));
        assert!(!super::is_interface_id("USBSTOR\\DISK&VEN_X&MI_00\\1"));
        assert!(!super::is_interface_id("SCSI\\DISK&VEN_SANDISK\\7&1"));
        assert!(!super::is_interface_id(""));

        let none = std::iter::empty::<&str>;
        assert!(super::is_hub("USB\\ROOT_HUB30\\7&27C23B87&0&0", none(), "USBHUB3"));
        assert!(super::is_hub("USB\\ROOT_HUB\\4&1", none(), ""));
        assert!(super::is_hub(
            "USB\\VID_05E3&PID_0610\\6&1",
            ["USB\\USB20_HUB", "USB\\Class_09&SubClass_00", "USB\\Class_09"].into_iter(),
            "",
        ));
        assert!(super::is_hub("USB\\VID_2109&PID_0817\\6&2", none(), "usbhub"));
        assert!(!super::is_hub(
            "USB\\VID_0781&PID_55BB\\MSFT3032",
            ["USB\\Class_08&SubClass_06&Prot_62", "USB\\Class_08"].into_iter(),
            "UASPStor",
        ));
        assert!(!super::is_hub("USB\\VID_05AC&PID_8600\\5&1", ["USB\\Class_0E"].into_iter(), "usbccgp"));
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
