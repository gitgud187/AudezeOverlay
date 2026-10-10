//! Temporary diagnostic. Changes only this application's compositor attributes,
//! invokes the independent pixel test, then restores the original attributes.
use std::{ffi::c_void, os::windows::process::CommandExt, process::Command};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Accent {
    state: u32,
    flags: u32,
    tint: u32,
    animation: u32,
}
#[repr(C)]
struct Attribute {
    kind: u32,
    data: *mut c_void,
    size: usize,
}
#[repr(C)]
struct BlurBehind {
    flags: u32,
    enabled: BOOL,
    region: HRGN,
    transition: BOOL,
}
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, kind: u32, data: *const c_void, size: u32) -> HRESULT;
    fn DwmGetWindowAttribute(hwnd: HWND, kind: u32, data: *mut c_void, size: u32) -> HRESULT;
    fn DwmEnableBlurBehindWindow(hwnd: HWND, data: *const BlurBehind) -> HRESULT;
    fn DwmFlush() -> HRESULT;
}
type AccentCall = unsafe extern "system" fn(HWND, *mut Attribute) -> BOOL;
unsafe fn accent_call(call: AccentCall, hwnd: HWND, accent: &mut Accent) -> bool {
    call(
        hwnd,
        &mut Attribute {
            kind: 19,
            data: (accent as *mut Accent).cast(),
            size: std::mem::size_of::<Accent>(),
        },
    )
    .as_bool()
}
unsafe fn set(hwnd: HWND, kind: u32, value: u32) -> HRESULT {
    DwmSetWindowAttribute(hwnd, kind, (&value as *const u32).cast(), 4)
}
unsafe fn get(hwnd: HWND, kind: u32, fallback: u32) -> u32 {
    let mut value = fallback;
    if DwmGetWindowAttribute(hwnd, kind, (&mut value as *mut u32).cast(), 4).is_ok() {
        value
    } else {
        fallback
    }
}
struct Restore {
    hwnd: HWND,
    call: AccentCall,
    accent: Accent,
    region: HRGN,
    had_region: bool,
    attributes: Vec<(u32, u32)>,
    style: isize,
}
impl Restore {
    unsafe fn apply(&self) {
        SetWindowLongPtrW(self.hwnd, GWL_STYLE, self.style);
        let _ = DwmEnableBlurBehindWindow(
            self.hwnd,
            &BlurBehind {
                flags: 1,
                enabled: BOOL(0),
                region: HRGN::default(),
                transition: BOOL(0),
            },
        );
        for &(kind, value) in &self.attributes {
            let _ = set(self.hwnd, kind, value);
        }
        let mut accent = self.accent;
        let _ = accent_call(self.call, self.hwnd, &mut accent);
        if self.had_region {
            let copy = CreateRectRgn(0, 0, 0, 0);
            let _ = CombineRgn(Some(copy), Some(self.region), None, RGN_COPY);
            if SetWindowRgn(self.hwnd, Some(copy), true) == 0 {
                let _ = DeleteObject(copy.into());
            }
        } else {
            let _ = SetWindowRgn(self.hwnd, None, true);
        }
        let _ = SetWindowPos(
            self.hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        let _ = PostMessageW(Some(self.hwnd), WM_APP + 6, WPARAM(0), LPARAM(0));
        let _ = DwmFlush();
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        unsafe {
            self.apply();
            let _ = DeleteObject(self.region.into());
        }
    }
}
unsafe fn blur_region(hwnd: HWND, radius: i32) -> HRESULT {
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let region = CreateRoundRectRgn(0, 0, r.right + 1, r.bottom + 1, radius * 2, radius * 2);
    let hr = DwmEnableBlurBehindWindow(
        hwnd,
        &BlurBehind {
            flags: 3,
            enabled: BOOL(1),
            region,
            transition: BOOL(0),
        },
    );
    let _ = DeleteObject(region.into());
    hr
}
fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hwnd = FindWindowW(w!("AudezeOverlayWindow"), None)?;
        let user = GetModuleHandleW(w!("user32.dll"))?;
        let call: AccentCall = std::mem::transmute(
            GetProcAddress(user, s!("SetWindowCompositionAttribute"))
                .ok_or_else(Error::from_thread)?,
        );
        let mut original = Accent::default();
        let read = GetProcAddress(user, s!("GetWindowCompositionAttribute"))
            .map(|p| {
                accent_call(
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, AccentCall>(p),
                    hwnd,
                    &mut original,
                )
            })
            .unwrap_or(false);
        if !read {
            if !std::env::args().any(|s| s == "--assume-acrylic") {
                return Err(Error::new(E_FAIL,"Cannot read original accent policy. Use --assume-acrylic only with our known default acrylic-enabled preview."));
            }
            original = Accent {
                state: 4,
                flags: 0,
                tint: 0x20101010,
                animation: 0,
            };
        }
        let region = CreateRectRgn(0, 0, 0, 0);
        let had_region = GetWindowRgn(hwnd, region) != GDI_REGION_TYPE(0);
        let restore = Restore {
            hwnd,
            call,
            accent: original,
            region,
            had_region,
            style: GetWindowLongPtrW(hwnd, GWL_STYLE),
            attributes: vec![
                (2, get(hwnd, 2, 1)),
                (33, get(hwnd, 33, 0)),
                (34, get(hwnd, 34, 0xffff_fffe)),
                (38, get(hwnd, 38, 1)),
            ],
        };
        println!(
            "Original accent {original:?}, region={had_region}, attributes={:?}",
            restore.attributes
        );
        let radius = (16. * GetDpiForWindow(hwnd).max(96) as f64 / 96.).round() as i32;
        let check = std::env::current_exe()
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?
            .with_file_name("check-blur.exe");
        let chosen = std::env::args().find_map(|s| s.strip_prefix("--only=").map(str::to_owned));
        for name in [
            "acrylic_blur_region",
            "blur3_flags2_region",
            "round_region_disabled_nc",
            "round_no_region_disabled_nc",
            "small_no_region_disabled_nc",
            "round_no_region_enabled_nc",
            "native_transient_round",
            "round_no_thickframe",
            "round_no_thickframe_region",
        ] {
            if chosen.as_ref().is_some_and(|s| s != name) {
                continue;
            }
            restore.apply();
            println!("\n=== {name} ===");
            match name {
                "acrylic_blur_region" => {
                    println!("DwmEnableBlurBehindWindow {:?}", blur_region(hwnd, radius))
                }
                "blur3_flags2_region" => {
                    let mut accent = Accent {
                        state: 3,
                        flags: 2,
                        tint: 0x20101010,
                        animation: 0,
                    };
                    println!("state3 accepted={}", accent_call(call, hwnd, &mut accent));
                    println!("DwmEnableBlurBehindWindow {:?}", blur_region(hwnd, radius));
                }
                _ => {
                    let corner = if name.starts_with("small") { 3 } else { 2 };
                    if !name.contains("round_region") && name != "round_no_thickframe_region" {
                        let _ = SetWindowRgn(hwnd, None, true);
                    }
                    if name.starts_with("round_no_thickframe") {
                        SetWindowLongPtrW(
                            hwnd,
                            GWL_STYLE,
                            restore.style & !(WS_THICKFRAME.0 as isize),
                        );
                    }
                    if name == "native_transient_round" {
                        let mut accent = Accent::default();
                        println!("disable accent={}", accent_call(call, hwnd, &mut accent));
                        println!("native backdrop={:?}", set(hwnd, 38, 3));
                    }
                    println!("corner preference={:?}", set(hwnd, 33, corner));
                    println!(
                        "nonclient policy={:?}",
                        set(
                            hwnd,
                            2,
                            if name.contains("enabled_nc") || name == "native_transient_round" {
                                2
                            } else {
                                1
                            }
                        )
                    );
                    println!("no border={:?}", set(hwnd, 34, 0xffff_fffe));
                }
            }
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            let _ = PostMessageW(Some(hwnd), WM_APP + 6, WPARAM(0), LPARAM(0));
            let _ = DwmFlush();
            let test_radius = if name.starts_with("small") {
                4
            } else if name.starts_with("round") || name == "native_transient_round" {
                8
            } else {
                16
            };
            let output = Command::new(&check)
                .arg("--corners")
                .arg(format!("--radius={test_radius}"))
                .creation_flags(0x08000000)
                .output()
                .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            for kind in ["baseline", "overlay"] {
                let _ = std::fs::copy(
                    format!("diagnostics/corners-{kind}.png"),
                    format!("diagnostics/probe-{name}-{kind}.png"),
                );
            }
        }
        drop(restore);
        println!("Original accent, region, and DWM attributes restored.");
        Ok(())
    }
}
