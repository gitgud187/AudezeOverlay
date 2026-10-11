//! Windows HID access through SetupAPI and the HID class driver.
use super::{
    control_report_sizes_plausible, Enumeration, Hid, InputStream, Interface, CONTROL_USAGE,
};
use crate::device::{profiles::Profile, selection::Candidate};
use windows::{
    core::PCWSTR,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*},
        Foundation::*,
        Storage::FileSystem::*,
    },
};
mod listener;
use listener::InputListener;
type Result<T> = std::result::Result<T, String>;

/// An open vendor-control interface; the handle is closed on drop.
struct Reader {
    handle: HANDLE,
    path: Vec<u16>,
    input_len: usize,
}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
impl Hid for Reader {
    fn write_report(&self, report: &[u8]) -> Result<()> {
        let mut written = 0;
        unsafe { WriteFile(self.handle, Some(report), Some(&mut written), None) }
            .map_err(|e| e.to_string())?;
        if written as usize != report.len() {
            return Err("Incomplete USB sound command".into());
        }
        Ok(())
    }
    fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()> {
        if !unsafe { HidD_GetInputReport(self.handle, report.as_mut_ptr().cast(), 64) } {
            return Err("USB connection lost. Reconnecting…".into());
        }
        Ok(())
    }
    fn listen(&self) -> Result<Box<dyn InputStream>> {
        Ok(Box::new(InputListener::open(&self.path, self.input_len)?))
    }
}

/// Lists the Audeze vendor-control interfaces Windows can open. Interfaces that
/// another component holds exclusively (such as consumer controls) are skipped.
pub(in crate::device) fn enumerate() -> Result<Enumeration> {
    unsafe {
        let guid = HidD_GetHidGuid();
        let list = SetupDiGetClassDevsW(
            Some(&guid),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
        .map_err(|e| e.to_string())?;
        let result = {
            let mut candidates = Vec::new();
            let mut wired_seen = Vec::new();
            for index in 0..256 {
                let mut iface = SP_DEVICE_INTERFACE_DATA {
                    cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                    ..Default::default()
                };
                if SetupDiEnumDeviceInterfaces(list, None, &guid, index, &mut iface).is_err() {
                    break;
                }
                let mut needed = 0;
                let _ = SetupDiGetDeviceInterfaceDetailW(
                    list,
                    &iface,
                    None,
                    0,
                    Some(&mut needed),
                    None,
                );
                if !(8..=32768).contains(&needed) {
                    continue;
                }
                let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
                let detail = storage.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(list, &iface, Some(detail), needed, None, None)
                    .is_err()
                {
                    continue;
                }
                let path = PCWSTR((*detail).DevicePath.as_ptr());
                let path_text = path.to_string().unwrap_or_default().to_ascii_lowercase();
                if !path_text.contains("vid_3329&pid_") {
                    continue;
                }
                if let Some(profile) = path_text
                    .split("vid_3329&pid_")
                    .nth(1)
                    .and_then(|s| s.get(..4))
                    .and_then(|s| u16::from_str_radix(s, 16).ok())
                    .and_then(|pid| Profile::from_usb(0x3329, pid))
                    .filter(|p| p.connection == crate::model::Connection::WiredUsb)
                {
                    if !wired_seen.contains(&profile) {
                        wired_seen.push(profile);
                    }
                }
                let handle = match CreateFileW(
                    path,
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                ) {
                    Ok(handle) => handle,
                    // Other collections (for example consumer controls) may
                    // be exclusively held by Windows. Continue to the vendor
                    // interface; never let an inaccessible sibling hide it.
                    Err(_) => continue,
                };
                let mut attrs = HIDD_ATTRIBUTES {
                    Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
                    ..Default::default()
                };
                let mut preparsed = PHIDP_PREPARSED_DATA::default();
                let mut caps = HIDP_CAPS::default();
                let profile = if HidD_GetAttributes(handle, &mut attrs) {
                    Profile::from_usb(attrs.VendorID, attrs.ProductID)
                } else {
                    None
                };
                let identity = profile.is_some();
                let usage = if identity && HidD_GetPreparsedData(handle, &mut preparsed) {
                    let status = HidP_GetCaps(preparsed, &mut caps);
                    let _ = HidD_FreePreparsedData(preparsed);
                    status.0 >= 0 && (caps.UsagePage as u32, caps.Usage as u32) == CONTROL_USAGE
                } else {
                    false
                };
                if !usage
                    || !control_report_sizes_plausible(
                        caps.InputReportByteLength as usize,
                        caps.OutputReportByteLength as usize,
                    )
                {
                    let _ = CloseHandle(handle);
                    continue;
                }
                let Some(profile) = profile else {
                    let _ = CloseHandle(handle);
                    continue;
                };
                let wide_path = path_text.encode_utf16().chain(Some(0)).collect();
                candidates.push(Candidate {
                    profile,
                    endpoint: Interface {
                        output_len: caps.OutputReportByteLength as usize,
                        hid: Box::new(Reader {
                            handle,
                            path: wide_path,
                            input_len: caps.InputReportByteLength as usize,
                        }),
                        id: path_text,
                    },
                });
            }
            Ok(Enumeration {
                candidates,
                wired_seen,
            })
        };
        let _ = SetupDiDestroyDeviceInfoList(list);
        result
    }
}
