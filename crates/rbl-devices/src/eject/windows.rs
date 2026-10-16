//! Windows ejection through Plug and Play, as "Safely Remove Hardware" does.
//!
//! The volume is only queried (a handle with no read or write access, which
//! needs no administrator rights) to learn which disk it is on. The eject
//! request then goes to the USB device that disk belongs to. Windows asks
//! every program holding a file on the stick to let go, flushes and
//! dismounts the file system, and refuses (a veto) while anything is still
//! open, so a busy stick is never forced.
#![allow(
    unsafe_code,
    reason = "Plug and Play and volume queries are Win32 FFI; every handle is owned and closed"
)]

use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    time::Duration,
};

use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            CM_Get_DevNode_Registry_PropertyW, CM_Get_Device_Interface_ListW,
            CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent,
            CM_Locate_DevNodeW, CM_Request_Device_EjectW, PNP_VetoTypeUnknown, CM_DEVCAP_REMOVABLE,
            CM_DRP_CAPABILITIES, CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_LOCATE_DEVNODE_NORMAL,
            CONFIGRET, CR_BUFFER_SMALL, CR_SUCCESS, PNP_VETO_TYPE,
        },
        Properties::{DEVPKEY_Device_InstanceId, DEVPROPTYPE},
    },
    Foundation::{INVALID_HANDLE_VALUE, MAX_PATH},
    Storage::FileSystem::{
        CreateFileW, FILE_DEVICE_DISK, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    },
    System::{
        Ioctl::{GUID_DEVINTERFACE_DISK, IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER},
        IO::DeviceIoControl,
    },
};

/// How often a vetoed request is repeated. Explorer and other programs can
/// take a moment to close their handles after the first query-remove.
const ATTEMPTS: usize = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);

pub(super) fn platform_eject(path: &Path) -> io::Result<()> {
    let letter = super::drive_letter(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Expected a USB drive root."))?;
    let volume = device_number(&wide(&format!("\\\\.\\{letter}:")))?;
    let disk = disk_node(volume.DeviceNumber)?;
    let target = removable_ancestor(disk)?;
    request_eject(target)
}

/// A NUL-terminated UTF-16 copy of `text`.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The disk number behind a volume or disk interface path. The handle asks
/// for no access, so this works without administrator rights and does not
/// keep the stick busy once it is dropped at the end of this call.
fn device_number(path: &[u16]) -> io::Result<STORAGE_DEVICE_NUMBER> {
    // SAFETY: NUL-terminated name and null optional pointers; synchronous handle.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a new, valid handle owned by this function.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut number = STORAGE_DEVICE_NUMBER {
        DeviceType: 0,
        DeviceNumber: 0,
        PartitionNumber: 0,
    };
    let mut returned = 0;
    // SAFETY: Live handle; the output buffer is a STORAGE_DEVICE_NUMBER of the size given.
    let ok = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            std::ptr::null(),
            0,
            (&raw mut number).cast(),
            u32::try_from(size_of::<STORAGE_DEVICE_NUMBER>()).unwrap_or(u32::MAX),
            &raw mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(number)
}

fn pnp_error(what: &str, code: CONFIGRET) -> io::Error {
    io::Error::other(format!(
        "Windows could not find the USB device ({what}, CONFIGRET {code})."
    ))
}

/// The device node of disk number `number`, found among the present disk
/// interfaces by asking each one for its number.
fn disk_node(number: u32) -> io::Result<u32> {
    let interfaces = disk_interfaces()?;
    for interface in super::multi_strings(&interfaces) {
        let mut path = interface.to_vec();
        path.push(0);
        let Ok(found) = device_number(&path) else {
            continue;
        };
        if found.DeviceType != FILE_DEVICE_DISK || found.DeviceNumber != number {
            continue;
        }
        let id = instance_id(&path)?;
        let mut node = 0;
        // SAFETY: NUL-terminated instance id and a valid out pointer.
        let code =
            unsafe { CM_Locate_DevNodeW(&raw mut node, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
        if code != CR_SUCCESS {
            return Err(pnp_error("locate disk", code));
        }
        return Ok(node);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "The USB volume is no longer connected.",
    ))
}

/// Every present disk interface path, as a multi-string. The list can grow
/// between asking for its size and reading it, so a short buffer is retried.
fn disk_interfaces() -> io::Result<Vec<u16>> {
    for _ in 0..ATTEMPTS {
        let mut length = 0;
        // SAFETY: Valid out pointer and interface GUID; no device id filter.
        let code = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &raw mut length,
                &GUID_DEVINTERFACE_DISK,
                std::ptr::null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if code != CR_SUCCESS {
            return Err(pnp_error("list disks", code));
        }
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: The buffer holds exactly `length` UTF-16 units.
        let code = unsafe {
            CM_Get_Device_Interface_ListW(
                &GUID_DEVINTERFACE_DISK,
                std::ptr::null(),
                buffer.as_mut_ptr(),
                length,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        match code {
            CR_SUCCESS => return Ok(buffer),
            CR_BUFFER_SMALL => {}
            _ => return Err(pnp_error("list disks", code)),
        }
    }
    Err(pnp_error("list disks", CR_BUFFER_SMALL))
}

/// The device instance id of a disk interface, NUL-terminated.
fn instance_id(interface: &[u16]) -> io::Result<Vec<u16>> {
    // A device instance id is at most 200 characters (MAX_DEVICE_ID_LEN).
    let mut buffer = vec![0u16; 256];
    let mut kind: DEVPROPTYPE = 0;
    let mut size = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: NUL-terminated interface path; the buffer holds `size` bytes.
    let code = unsafe {
        CM_Get_Device_Interface_PropertyW(
            interface.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &raw mut kind,
            buffer.as_mut_ptr().cast(),
            &raw mut size,
            0,
        )
    };
    if code != CR_SUCCESS {
        return Err(pnp_error("disk id", code));
    }
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    buffer.truncate(end);
    buffer.push(0);
    Ok(buffer)
}

/// Whether Plug and Play calls the device removable (`CM_DEVCAP_REMOVABLE`),
/// which is what puts a device in "Safely Remove Hardware".
fn is_removable(node: u32) -> bool {
    let mut capabilities = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap_or(4);
    // SAFETY: The buffer is one u32 of the size given; the type out pointer is optional.
    let code = unsafe {
        CM_Get_DevNode_Registry_PropertyW(
            node,
            CM_DRP_CAPABILITIES,
            std::ptr::null_mut(),
            (&raw mut capabilities).cast(),
            &raw mut size,
            0,
        )
    };
    code == CR_SUCCESS && capabilities & CM_DEVCAP_REMOVABLE != 0
}

/// What to eject for a disk: its parent (the USB device for a stick, so the
/// whole stick goes, as with "Safely Remove Hardware"), or the disk itself
/// when only the disk is removable. A disk with neither, such as one inside
/// the computer, is refused rather than ejecting its controller.
fn removable_ancestor(disk: u32) -> io::Result<u32> {
    let mut parent = 0;
    // SAFETY: Valid out pointer and a device node from CM_Locate_DevNodeW.
    if unsafe { CM_Get_Parent(&raw mut parent, disk, 0) } == CR_SUCCESS && is_removable(parent) {
        return Ok(parent);
    }
    if is_removable(disk) {
        return Ok(disk);
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows does not offer this drive for safe removal, so it cannot be ejected here.",
    ))
}

/// Asks Windows to prepare `node` for removal, repeating a veto briefly.
fn request_eject(node: u32) -> io::Result<()> {
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RETRY_DELAY);
        }
        let mut veto: PNP_VETO_TYPE = PNP_VetoTypeUnknown;
        let mut name = [0u16; MAX_PATH as usize];
        // SAFETY: Valid device node; the veto name buffer holds MAX_PATH units.
        // Passing a name buffer also stops Windows showing its own balloon.
        let code = unsafe {
            CM_Request_Device_EjectW(node, &raw mut veto, name.as_mut_ptr(), MAX_PATH, 0)
        };
        if code == CR_SUCCESS && veto == PNP_VetoTypeUnknown {
            return Ok(());
        }
        let end = name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len());
        let name = String::from_utf16_lossy(&name[..end]);
        tracing::info!(attempt, code, veto, %name, "Windows vetoed the USB eject");
        last = super::veto_message(veto, &name);
    }
    Err(io::Error::other(last))
}
