//! App-only transition regression harness; not linked into the product.
//! Run with the sound overlay open. Does not send headset commands or edit settings.
#[path = "../platform/blur.rs"]
mod blur;

use std::{
    os::windows::process::CommandExt, path::PathBuf, process::Command, thread::sleep,
    time::Duration,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

const OPEN: u32 = WM_APP + 3;
const DRAW: u32 = WM_APP + 6;

struct Restore {
    hwnd: HWND,
    rect: RECT,
}
impl Drop for Restore {
    fn drop(&mut self) {
        unsafe {
            // OPEN always returns to Sound and interactive mode. Reposition
            // afterwards because OPEN uses the user's persisted placement.
            let _ = SendMessageW(self.hwnd, OPEN, Some(WPARAM(0)), Some(LPARAM(0)));
            let _ = blur::apply(self.hwnd, true);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                self.rect.left,
                self.rect.top,
                self.rect.right - self.rect.left,
                self.rect.bottom - self.rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = PostMessageW(Some(self.hwnd), DRAW, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn redraw(hwnd: HWND) {
    let _ = PostMessageW(Some(hwnd), DRAW, WPARAM(0), LPARAM(0));
    sleep(Duration::from_millis(200));
}

unsafe fn click(hwnd: HWND, x: f32, y: f32) {
    let x = x.round() as i32;
    let y = y.round() as i32;
    let point = LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize);
    let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(point));
    let _ = SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point));
    redraw(hwnd);
}

fn check(exe: &PathBuf, name: &str) -> Result<bool> {
    println!("\n=== {name} ===");
    let output = Command::new(exe)
        .args(["--corners", "--radius=8"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    for kind in ["baseline", "overlay"] {
        let _ = std::fs::copy(
            format!("diagnostics/corners-{kind}.png"),
            format!("diagnostics/transition-{name}-{kind}.png"),
        );
    }
    let pass = output.status.success() && stdout.contains("corner_blur_check=PASS");
    println!("transition_{name}={}", if pass { "PASS" } else { "FAIL" });
    Ok(pass)
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hwnd = FindWindowW(w!("AudezeOverlayWindow"), None)?;
        let mut original = RECT::default();
        GetWindowRect(hwnd, &mut original)?;
        let restore = Restore {
            hwnd,
            rect: original,
        };
        let config = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("AudezeOverlay/settings.json"));
        let before = config.as_ref().and_then(|p| std::fs::read(p).ok());
        let exe = std::env::current_exe()
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?
            .with_file_name("check-blur.exe");
        let _ = SendMessageW(hwnd, OPEN, Some(WPARAM(0)), Some(LPARAM(0)));
        redraw(hwnd);
        let mut results = Vec::new();
        results.push(("initial_sound", check(&exe, "initial_sound")?));

        println!("blur_off API accepted={}", blur::apply(hwnd, false));
        redraw(hwnd);
        results.push(("blur_off", check(&exe, "blur_off")?));
        println!("blur_on API accepted={}", blur::apply(hwnd, true));
        redraw(hwnd);
        results.push(("blur_on", check(&exe, "blur_on")?));

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let work = info.rcWork;
        let width = (original.right - original.left + 32).min(work.right - work.left);
        let height = (original.bottom - original.top + 32).min(work.bottom - work.top);
        let x = (original.left + 40).clamp(work.left, (work.right - width).max(work.left));
        let y = (original.top + 40).clamp(work.top, (work.bottom - height).max(work.top));
        // SetWindowPos triggers normal WM_SIZE/region refresh but deliberately
        // does not send WM_EXITSIZEMOVE, which is the persistence trigger.
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
        redraw(hwnd);
        results.push(("moved_resized", check(&exe, "moved_resized")?));
        SetWindowPos(
            hwnd,
            None,
            original.left,
            original.top,
            original.right - original.left,
            original.bottom - original.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
        redraw(hwnd);
        results.push(("restored_size", check(&exe, "restored_size")?));

        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client)?;
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
        click(hwnd, client.right as f32 - 88. * scale, 36. * scale); // Settings cog.
        results.push(("settings", check(&exe, "settings")?));
        GetClientRect(hwnd, &mut client)?;
        let settings_scale = (client.right as f32 / 900.).min(client.bottom as f32 / 750.);
        click(hwnd, 770. * settings_scale, 36. * settings_scale); // Back to sound.
        results.push(("back_to_sound", check(&exe, "back_to_sound")?));

        GetClientRect(hwnd, &mut client)?;
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
        click(hwnd, client.right as f32 - 38. * scale, 36. * scale); // Minimize to tray.
        let minimized = !IsWindowVisible(hwnd).as_bool();
        println!("minimize_hid_window={minimized}");
        results.push(("minimize_hid_window", minimized));
        let _ = SendMessageW(hwnd, OPEN, Some(WPARAM(0)), Some(LPARAM(0)));
        redraw(hwnd);
        results.push(("reopened", check(&exe, "reopened")?));

        drop(restore);
        sleep(Duration::from_millis(200));
        let after = config.as_ref().and_then(|p| std::fs::read(p).ok());
        let unchanged = before == after;
        results.push(("config_unchanged", unchanged));
        let mut final_rect = RECT::default();
        GetWindowRect(hwnd, &mut final_rect)?;
        let placement = (
            final_rect.left,
            final_rect.top,
            final_rect.right,
            final_rect.bottom,
        ) == (original.left, original.top, original.right, original.bottom);
        results.push(("placement_restored", placement));
        println!("\nTransition checks: {results:?}");
        println!(
            "all_transitions={}",
            if results.iter().all(|(_, ok)| *ok) {
                "PASS"
            } else {
                "FAIL"
            }
        );
        println!(
            "Sound view open, original placement restored, blur enabled; no headset commands sent."
        );
        Ok(())
    }
}
