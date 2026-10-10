//! Standalone visual test harness. Screenshot code is not linked into the app.
#[path = "../platform/blur.rs"]
mod blur;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::sleep,
    time::Duration,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*,
    },
};
static CORNER_PATTERN: AtomicBool = AtomicBool::new(false);

#[link(name = "dwmapi")]
extern "system" {
    fn DwmFlush() -> HRESULT;
}

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_PAINT {
        let mut p = PAINTSTRUCT::default();
        let dc = BeginPaint(h, &mut p);
        let mut r = RECT::default();
        let _ = GetClientRect(h, &mut r);
        for y in (0..r.bottom).step_by(8) {
            for x in (0..r.right).step_by(8) {
                let b = CreateSolidBrush(COLORREF(if CORNER_PATTERN.load(Ordering::Relaxed) {
                    if (x / 8 + y / 8) % 2 == 0 {
                        0xff00ff
                    } else {
                        0x00ff00
                    }
                } else if (x / 8 + y / 8) % 2 == 0 {
                    if x < 300 {
                        0x2020dd
                    } else {
                        0xdd8020
                    }
                } else {
                    0x202020
                }));
                FillRect(
                    dc,
                    &RECT {
                        left: x,
                        top: y,
                        right: x + 8,
                        bottom: y + 8,
                    },
                    b,
                );
                let _ = DeleteObject(b.into());
            }
        }
        let _ = EndPaint(h, &p);
        LRESULT(0)
    } else {
        DefWindowProcW(h, m, w, l)
    }
}
unsafe fn pixels(r: RECT, path: &str) -> Result<tiny_skia::Pixmap> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let screen = GetDC(None);
    let dc = CreateCompatibleDC(Some(screen));
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: 40,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut ptr = std::ptr::null_mut();
    let b = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut ptr, None, 0)?;
    let old = SelectObject(dc, b.into());
    BitBlt(
        dc,
        0,
        0,
        w,
        h,
        Some(screen),
        r.left,
        r.top,
        SRCCOPY | CAPTUREBLT,
    )?;
    let src = std::slice::from_raw_parts(ptr as *const u8, (w * h * 4) as usize);
    let mut pix = tiny_skia::Pixmap::new(w as u32, h as u32).unwrap();
    for (s, d) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(pix.data_mut().as_chunks_mut::<4>().0.iter_mut())
    {
        d.copy_from_slice(&[s[2], s[1], s[0], 255]);
    }
    let saved = pix.save_png(path);
    SelectObject(dc, old);
    let _ = DeleteObject(b.into());
    let _ = DeleteDC(dc);
    ReleaseDC(None, screen);
    if let Err(error) = saved {
        eprintln!("Could not save {path}: {error}");
    }
    Ok(pix)
}
unsafe fn capture(r: RECT, path: &str) -> Result<f64> {
    let pix = pixels(r, path)?;
    let (w, h) = (pix.width() as i32, pix.height() as i32);
    let src = pix.data();
    let mut sum = 0f64;
    let mut squared = 0f64;
    let mut count = 0f64;
    let sx = w as f32 / 620.;
    let sy = h as f32 / 500.;
    for y in (20. * sy) as i32..(60. * sy) as i32 {
        for x in (330. * sx) as i32..(425. * sx) as i32 {
            let v = src[((y * w + x) * 4 + 2) as usize] as f64;
            sum += v;
            squared += v * v;
            count += 1.;
        }
    }
    Ok(squared / count - (sum / count).powi(2))
}

// Returns a signed distance from a pixel center to the intended circular
// rounded rectangle. Positive distances are outside the visible surface.
fn rounded_distance(x: f64, y: f64, w: f64, h: f64, radius: f64) -> f64 {
    let radius = radius.min(w / 2.).min(h / 2.);
    let qx = (x - w / 2.).abs() - (w / 2. - radius);
    let qy = (y - h / 2.).abs() - (h / 2. - radius);
    qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius
}

#[derive(Default)]
struct Difference {
    sampled: usize,
    changed: usize,
    max_channel: u8,
    color_contamination: usize,
    max_added_channel: u8,
    pattern_samples: usize,
}
impl Difference {
    fn add(&mut self, a: &[u8], b: &[u8]) {
        self.sampled += 1;
        self.pattern_samples += usize::from(a[..3] == [255, 0, 255] || a[..3] == [0, 255, 0]);
        let delta = (0..3).map(|n| a[n].abs_diff(b[n])).max().unwrap_or(0);
        self.changed += usize::from(delta != 0);
        self.max_channel = self.max_channel.max(delta);
        // The baseline is saturated magenta/green. A neutral shadow only
        // darkens its nonzero channels; blur mixes in formerly zero channels.
        let added = (0..3)
            .filter(|&n| a[n] == 0)
            .map(|n| b[n])
            .max()
            .unwrap_or(0);
        self.color_contamination += usize::from(added > 1);
        self.max_added_channel = self.max_added_channel.max(added);
    }
    fn print(&self, name: &str) {
        println!(
            "{name}: sampled={} changed={} max_channel_difference={} color_contamination={} max_added_channel={}",
            self.sampled, self.changed, self.max_channel, self.color_contamination, self.max_added_channel
        );
    }
}

struct CornerRestore {
    overlay: HWND,
    pattern: HWND,
    region: HRGN,
    visible: bool,
}
impl Drop for CornerRestore {
    fn drop(&mut self) {
        unsafe {
            let _ = ShowWindow(
                self.overlay,
                if self.visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                },
            );
            if !self.region.is_invalid() {
                let _ = DeleteObject(self.region.into());
            }
            if !self.pattern.is_invalid() {
                let _ = DestroyWindow(self.pattern);
            }
        }
    }
}

unsafe fn corners(overlay: HWND, r: RECT, pattern: HWND, area: RECT) -> Result<()> {
    let region = CreateRectRgn(0, 0, 0, 0);
    let shape = GetWindowRgn(overlay, region);
    let _restore = CornerRestore {
        overlay,
        pattern,
        region,
        visible: IsWindowVisible(overlay).as_bool(),
    };
    // This test does not change the accent policy, position, size, or settings.
    let _ = ShowWindow(overlay, SW_HIDE);
    let _ = UpdateWindow(pattern);
    let _ = DwmFlush();
    sleep(Duration::from_millis(250));
    let baseline = pixels(area, "diagnostics/corners-baseline.png")?;
    let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
    let _ = PostMessageW(Some(overlay), WM_APP + 6, WPARAM(0), LPARAM(0));
    sleep(Duration::from_millis(500));
    let _ = DwmFlush();
    let shown = pixels(area, "diagnostics/corners-overlay.png")?;
    let mut after = RECT::default();
    GetWindowRect(overlay, &mut after)?;
    if (r.left, r.top, r.right, r.bottom) != (after.left, after.top, after.right, after.bottom) {
        return Err(Error::new(
            E_FAIL,
            "Overlay moved during corner comparison; rerun without moving it.",
        ));
    }
    let logical_radius = std::env::args()
        .find_map(|s| {
            s.strip_prefix("--radius=")
                .and_then(|r| r.parse::<f64>().ok())
        })
        .unwrap_or(8.)
        .clamp(1., 64.);
    let radius =
        windows::Win32::UI::HiDpi::GetDpiForWindow(overlay).max(96) as f64 / 96. * logical_radius;
    let width = (r.right - r.left) as f64;
    let height = (r.bottom - r.top) as f64;
    let has_region = shape != GDI_REGION_TYPE(0);
    let mut circle = Difference::default();
    let mut actual = Difference::default();
    let mut exterior = Difference::default();
    let mut mask_corners = Difference::default();
    let mut interior = Difference::default();
    for y in 0..baseline.height() as i32 {
        for x in 0..baseline.width() as i32 {
            let local_x = area.left + x - r.left;
            let local_y = area.top + y - r.top;
            let index = ((y as u32 * baseline.width() + x as u32) * 4) as usize;
            let a = &baseline.data()[index..index + 4];
            let b = &shown.data()[index..index + 4];
            if rounded_distance(
                local_x as f64 + 0.5,
                local_y as f64 + 0.5,
                width,
                height,
                radius,
            ) < -4.
            {
                interior.add(a, b);
            }
            // Exclude a one-pixel boundary band to avoid counting valid AA.
            if rounded_distance(
                local_x as f64 + 0.5,
                local_y as f64 + 0.5,
                width,
                height,
                radius,
            ) > 1.
            {
                circle.add(a, b);
                if local_x >= 0 && local_y >= 0 && local_x < width as i32 && local_y < height as i32
                {
                    mask_corners.add(a, b);
                }
            }
            if local_x < -1 || local_y < -1 || local_x > width as i32 || local_y > height as i32 {
                exterior.add(a, b);
            }
            if has_region
                && (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| !PtInRegion(region, local_x + dx, local_y + dy).as_bool())
                })
            {
                actual.add(a, b);
            }
        }
    }
    println!("corner radius={radius:.2}px region_present={has_region}; blur left unchanged; one-pixel AA band excluded");
    circle.print("outside_intended_circle");
    mask_corners.print("corner_cutouts_only");
    exterior.print("outside_window_rectangle");
    actual.print("outside_actual_window_region");
    interior.print("inside_window_sanity_check");
    for (x, y) in [(0, 0), (1, 1), (3, 3), (7, 0), (0, 7), (-5, -5)] {
        let px = r.left - area.left + x;
        let py = r.top - area.top + y;
        if px >= 0 && py >= 0 && px < baseline.width() as i32 && py < baseline.height() as i32 {
            let i = ((py as u32 * baseline.width() + px as u32) * 4) as usize;
            println!(
                "corner_pixel({x},{y}): {:?} -> {:?}",
                &baseline.data()[i..i + 3],
                &shown.data()[i..i + 3]
            );
        }
    }
    println!(
        "corner_blur_check={} (neutral Windows shadow allowed)",
        if circle.sampled > 0
            && circle.pattern_samples == circle.sampled
            && circle.color_contamination == 0
            && (!has_region || actual.color_contamination == 0)
            && interior.changed > 100
            && IsWindowVisible(overlay).as_bool()
        {
            "PASS"
        } else {
            "FAIL"
        }
    );
    println!(
        "outside_pixels_identical={}; baseline_pattern_valid={}",
        circle.changed == 0,
        circle.pattern_samples == circle.sampled
    );
    Ok(())
}

fn main() -> Result<()> {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        let overlay = FindWindowW(w!("AudezeOverlayWindow"), None)?;
        let mut r = RECT::default();
        GetWindowRect(overlay, &mut r)?;
        let corner_mode = std::env::args().any(|s| s == "--corners");
        CORNER_PATTERN.store(corner_mode, Ordering::Relaxed);
        if std::env::args().any(|s| s == "--capture-only") {
            let _ = std::fs::create_dir_all("diagnostics");
            capture(r, "diagnostics/live-overlay.png")?;
            return Ok(());
        }
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("AudezeBlurTestPattern"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let area = if corner_mode {
            RECT {
                left: (r.left - 32).max(GetSystemMetrics(SM_XVIRTUALSCREEN)),
                top: (r.top - 32).max(GetSystemMetrics(SM_YVIRTUALSCREEN)),
                right: (r.right + 32).min(
                    GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                ),
                bottom: (r.bottom + 32).min(
                    GetSystemMetrics(SM_YVIRTUALSCREEN) + GetSystemMetrics(SM_CYVIRTUALSCREEN),
                ),
            }
        } else {
            r
        };
        let pattern = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("AudezeBlurTestPattern"),
            w!("Overlay blur test pattern"),
            WS_POPUP,
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            None,
            None,
            Some(instance),
            None,
        )?;
        let _ = ShowWindow(pattern, SW_HIDE);
        SetWindowPos(
            pattern,
            Some(overlay),
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        let _ = UpdateWindow(pattern);
        let _ = std::fs::create_dir_all("diagnostics");
        if corner_mode {
            return corners(overlay, r, pattern, area);
        }
        let off_ok = blur::apply(overlay, false);
        let _ = PostMessageW(Some(overlay), WM_APP + 6, WPARAM(0), LPARAM(0));
        sleep(Duration::from_millis(1200));
        let off = capture(r, "diagnostics/blur-off.png")?;
        let on_ok = blur::apply(overlay, true);
        let _ = PostMessageW(Some(overlay), WM_APP + 6, WPARAM(0), LPARAM(0));
        sleep(Duration::from_millis(1200));
        let on = capture(r, "diagnostics/blur-on.png")?;
        println!("off accepted={off_ok}; on accepted={on_ok}; background variance off={off:.2}, on={on:.2}; softening={:.1}%",100.*(1.-on/off.max(1.)));
        blur::apply(overlay, true);
        DestroyWindow(pattern)?;
        Ok(())
    }
}
