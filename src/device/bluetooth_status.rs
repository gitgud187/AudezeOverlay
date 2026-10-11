//! Passive Windows Classic Bluetooth status, not a headset control transport.
use crate::model::HeadsetFamily;
use windows::Win32::Devices::Bluetooth::*;

fn family(name: &str) -> Option<HeadsetFamily> {
    match name.trim().to_ascii_lowercase().as_str() {
        "audeze maxwell" | "maxwell" => Some(HeadsetFamily::Maxwell),
        "audeze maxwell 2" | "audeze maxwell2" | "maxwell 2" | "maxwell2" => {
            Some(HeadsetFamily::Maxwell2)
        }
        "audeze maxwell 2 anc" | "audeze maxwell2 anc" | "maxwell 2 anc" | "maxwell2 anc" => {
            Some(HeadsetFamily::Maxwell2Anc)
        }
        _ => None,
    }
}
struct Search(HBLUETOOTH_DEVICE_FIND);
impl Drop for Search {
    fn drop(&mut self) {
        unsafe {
            let _ = BluetoothFindDeviceClose(self.0);
        }
    }
}
/// Read Windows' existing connection cache. Never starts inquiry or pairs a device.
/// Names are display-only evidence; they never grant USB command capabilities.
pub(super) fn connected() -> Option<HeadsetFamily> {
    let parameters = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnConnected: true.into(),
        fIssueInquiry: false.into(),
        ..Default::default()
    };
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: correctly sized owned structs; the search handle is closed by RAII.
    unsafe {
        let search = Search(BluetoothFindFirstDevice(&parameters, &mut info).ok()?);
        let mut found = None;
        loop {
            let end = info
                .szName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.szName.len());
            if info.fConnected.as_bool() {
                if let Some(model) = family(&String::from_utf16_lossy(&info.szName[..end])) {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(model);
                }
            }
            if BluetoothFindNextDevice(search.0, &mut info).is_err() {
                break;
            }
        }
        found
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_recognized_names_are_used_for_display() {
        assert_eq!(
            family("Audeze Maxwell2 ANC"),
            Some(HeadsetFamily::Maxwell2Anc)
        );
        assert_eq!(family("Maxwell 2"), Some(HeadsetFamily::Maxwell2));
        assert_eq!(family("Maxwell"), Some(HeadsetFamily::Maxwell));
        assert_eq!(family("Tim's Maxwell-shaped speaker"), None);
    }
}
