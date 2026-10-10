//! Read-only, explicitly whitelisted headset status probes.
use hidapi::HidApi;
use std::time::{Duration, Instant};
use windows::{
    core::PCWSTR,
    Win32::{Devices::HumanInterfaceDevice::*, Foundation::*, Storage::FileSystem::*},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    HidApi::disable_device_discovery();
    let mut api = HidApi::new()?;
    api.add_devices(0x3329, 0x4ba9)?;
    for info in api.device_list().filter(|d| d.vendor_id() == 0x3329) {
        println!(
            "{:04x}:{:04x} usage {:04x}/{:04x} interface {} {}",
            info.vendor_id(),
            info.product_id(),
            info.usage_page(),
            info.usage(),
            info.interface_number(),
            info.path().to_string_lossy()
        );
        if !std::env::args().any(|s| s == "--query") || info.usage_page() < 0xff00 {
            continue;
        }
        let d = info.open_device(&api)?;
        let path: Vec<u16> = info
            .path()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )?
        };
        for (name, body) in [
            ("mix", vec![5, 0x5a, 4, 0, 0x83, 0x2c, 0x0b, 0]),
            ("noise", vec![5, 0x5a, 4, 0, 1, 9, 0x24, 0]),
            ("sidetone", vec![5, 0x5a, 4, 0, 1, 9, 0x2c, 0]),
            ("anc", vec![5, 0x5a, 3, 0, 0x30, 0x2e, 2]),
            ("mute", vec![5, 0x5a, 4, 0, 1, 9, 0x22, 0]),
            ("battery", vec![5, 0x5a, 3, 0, 0xd6, 0x0c, 0]),
        ] {
            let mut report = [0u8; 62];
            report[0] = 6;
            report[1] = body.len() as u8;
            report[2] = 0x80;
            report[3..3 + body.len()].copy_from_slice(&body);
            let started = Instant::now();
            d.write(&report)?;
            while started.elapsed() < Duration::from_millis(150) {
                std::thread::sleep(Duration::from_millis(10));
                let mut buf = [0u8; 64];
                buf[0] = 7;
                if !unsafe { HidD_GetInputReport(handle, buf.as_mut_ptr().cast(), 64) } {
                    return Err(windows::core::Error::from_thread().into());
                };
                if buf[1] > 0 {
                    println!(
                        "{name} {}ms {:02x?}",
                        started.elapsed().as_millis(),
                        &buf[..32]
                    );
                }
            }
        }
        unsafe {
            CloseHandle(handle)?;
        }
    }
    Ok(())
}
