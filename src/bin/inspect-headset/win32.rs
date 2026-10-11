//! Read-only, explicitly whitelisted headset status probes.
use hidapi::HidApi;
use std::time::{Duration, Instant};
use windows::{
    core::PCWSTR,
    Win32::{Devices::HumanInterfaceDevice::*, Foundation::*, Storage::FileSystem::*},
};
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The official backend requests 62 bytes. Compare it with our 64-byte
    // request only in this explicitly invoked, bounded read-only diagnostic.
    let report_size = if std::env::args().any(|s| s == "--official-report-size") {
        62
    } else {
        64
    };
    HidApi::disable_device_discovery();
    let mut api = HidApi::new()?;
    api.add_devices(0x3329, 0x4ba9)?;
    api.add_devices(0x3329, 0x4baa)?;
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
        if !std::env::args().any(|s| s == "--query")
            || info.usage_page() != 0xff13
            || info.usage() != 1
        {
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
            // Official 1.2.48 backend readPoweredOn(bool&), 0x140022670.
            // This reads status; it does not switch headset power or pairing.
            ("powered-on", vec![5, 0x5a, 2, 0, 0x80, 0x2c]),
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
            report[2] = if info.product_id() == 0x4baa { 0 } else { 0x80 };
            report[3..3 + body.len()].copy_from_slice(&body);
            let started = Instant::now();
            d.write(&report)?;
            let mut reads = 0;
            let mut replies = 0;
            while started.elapsed() < Duration::from_millis(150) {
                std::thread::sleep(Duration::from_millis(10));
                let mut buf = [0u8; 64];
                buf[0] = 7;
                if !unsafe { HidD_GetInputReport(handle, buf.as_mut_ptr().cast(), report_size) } {
                    return Err(windows::core::Error::from_thread().into());
                };
                reads += 1;
                if buf[1] > 0 {
                    replies += 1;
                }
                if buf[1] > 0 || reads == 1 {
                    println!(
                        "{name} {}ms {:02x?}",
                        started.elapsed().as_millis(),
                        &buf[..32]
                    );
                }
            }
            println!(
                "{name}: report size={report_size}, reads={reads}, populated replies={replies}"
            );
        }
        unsafe {
            CloseHandle(handle)?;
        }
    }
    Ok(())
}
