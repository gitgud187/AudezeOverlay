//! Owns GDI surfaces and presents rasterized frames to the layered window.
use super::*;
pub(super) struct Bitmap {
    dc: HDC,
    pub(super) bitmap: HBITMAP,
    old: HGDIOBJ,
    size: SIZE,
    bits: std::ptr::NonNull<u8>,
}
impl Bitmap {
    unsafe fn new(w: u32, h: u32) -> Option<Self> {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let Some(bits) = std::ptr::NonNull::new(bits.cast::<u8>()) else {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, bitmap.into());
        if old.is_invalid() || old.0 as isize == -1 {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            return None;
        }
        Some(Self {
            dc,
            bitmap,
            old,
            size: SIZE {
                cx: w as i32,
                cy: h as i32,
            },
            bits,
        })
    }

    fn write(&mut self, pixels: &[u8]) {
        let len = (self.size.cx as usize) * (self.size.cy as usize) * 4;
        // Both dimensions come from this frame's client rectangle.
        assert_eq!(pixels.len(), len);
        // SAFETY: the owned top-down 32-bit DIB has exactly this byte capacity.
        // Its backing storage outlives this borrow; only the UI thread uses it.
        // Finish GDI's previous use before writing the retained DIB directly.
        let dst = unsafe {
            let _ = GdiFlush();
            std::slice::from_raw_parts_mut(self.bits.as_ptr(), len)
        };
        for (src, dst) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dst.as_chunks_mut::<4>().0.iter_mut())
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
pub(super) fn displayed_mix(app: &App) -> f32 {
    if let Some(t) = app.mix_started {
        let x = (t.elapsed().as_secs_f32() * 1000. / MIX_ANIMATION_MS as f32).min(1.);
        app.mix_from + (app.mix_to - app.mix_from) * (1. - (1. - x).powi(3))
    } else {
        app.mix_to
    }
}
pub(super) unsafe fn render(app: &mut App) {
    render_frame(app, false);
}
pub(super) unsafe fn render_frame(app: &mut App, prepare_hidden: bool) {
    app.draw_queued = false;
    if !IsWindowVisible(app.hwnd).as_bool() && !app.capture && !prepare_hidden {
        return;
    }
    let mut r = RECT::default();
    if GetClientRect(app.hwnd, &mut r).is_err() {
        return;
    }
    let (w, h) = (
        (r.right - r.left).max(1) as u32,
        (r.bottom - r.top).max(1) as u32,
    );
    if w > 8192 || h > 8192 {
        return;
    }
    app.last_frame = Some(Instant::now());
    let mut scale = GetDpiForWindow(app.hwnd).max(96) as f32 / 96.;
    if app.settings.is_some() {
        scale = (w as f32 / 900.).min(h as f32 / 750.);
    }
    let label = app
        .monitors
        .iter()
        .find(|m| m.key == app.config.monitor)
        .map(|m| m.label.as_str())
        .unwrap_or("Auto");
    let mix = displayed_mix(app);
    let popup_config = app
        .popup_cards
        .map(|mask| app.config.notification_layout(mask));
    let config = popup_config.as_ref().unwrap_or(&app.config);
    let animation = app.presentation.frame(
        dashboard::card_boxes(config, w as f32 / scale, h as f32 / scale),
        &app.sound,
        matches!(app.drag, Drag::Mix),
        app.config.animations
            && app.settings.is_none()
            && !app.capture
            && !app.smoke
            && !prepare_hidden
            && IsWindowVisible(app.hwnd).as_bool(),
        Instant::now(),
    );
    app.motion_active = animation.active;
    let view = View {
        eq_editor: Some(&app.eq_editor),
        config,
        sound: &app.sound,
        settings: app.settings,
        edit: app.edit,
        hover: app.hover,
        note: app.note.text(),
        recording: app.recording,
        recorded_key: app.recorded_key,
        notification_options: app.notification_options,
        popup_cards: app.popup_cards,
        number_edit: app.number_edit.as_ref(),
        mix,
        demo: app.demo,
        monitor_label: label,
        drag_preview: app.drag_preview,
        animation: Some(&animation),
    };
    let scene = if let Some(raster) = app.raster.take() {
        dashboard::render_reusing(&mut app.painter, w, h, scale, &view, Some(raster))
    } else {
        dashboard::render(&mut app.painter, w, h, scale, &view)
    };
    app.hits = scene.hits;
    app.cards = scene.cards;
    if app.motion_active {
        start_animation_timer(app);
    }
    if app.capture {
        let _ = std::fs::create_dir_all("diagnostics");
        let name = if let Some(section) = app.settings {
            format!("diagnostics/settings-v3-{section}.png")
        } else {
            "diagnostics/overlay-v3.png".into()
        };
        let _ = scene.pix.save_png(name);
        if std::env::args().any(|s| s == "--render-only") {
            let _ = PostMessageW(Some(app.hwnd), WM_APP + 7, WPARAM(0), LPARAM(0));
        }
    }
    if !app
        .bitmap
        .as_ref()
        .is_some_and(|b| b.size.cx == w as i32 && b.size.cy == h as i32)
    {
        let Some(bitmap) = Bitmap::new(w, h) else {
            return;
        };
        app.bitmap = Some(bitmap);
    }
    if let Some(bitmap) = &mut app.bitmap {
        bitmap.write(scene.pix.data());
    }
    app.raster = Some(scene.pix);
    present(app);
    if let Some(delivery) = app.telemetry_at {
        device::trace_present(delivery, app.alpha, app.sound.mix.map(|m| m.1));
        if app.alpha >= 230 {
            app.telemetry_at = None;
        }
    }
}
pub(super) unsafe fn present(app: &App) {
    if let Some(b) = &app.bitmap {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: app.alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            app.hwnd,
            None,
            None,
            Some(&b.size),
            Some(b.dc),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
    }
}
pub(super) unsafe fn clip_window(app: &App) {
    let mut r = RECT::default();
    if GetClientRect(app.hwnd, &mut r).is_err() || r.right <= 0 || r.bottom <= 0 {
        return;
    }
    let scale = if app.settings.is_some() {
        (r.right as f32 / 900.).min(r.bottom as f32 / 750.)
    } else {
        GetDpiForWindow(app.hwnd).max(96) as f32 / 96.
    };
    let diameter = (16. * scale).round() as i32;
    let region = CreateRoundRectRgn(0, 0, r.right + 1, r.bottom + 1, diameter, diameter);
    if !region.is_invalid() && SetWindowRgn(app.hwnd, Some(region), true) == 0 {
        // On success Windows owns the region; on failure it is still ours.
        let _ = DeleteObject(region.into());
    }
}
pub(super) unsafe fn create_tray_icon(painter: &Painter) -> HICON {
    let size = 32;
    let Some(mut pix) = tiny_skia::Pixmap::new(size, size) else {
        return HICON::default();
    };
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(245, 245, 245, 255);
    // The wordmark SVG contains the symbol at x=0..29, y=0..24.
    // Draw those original vector paths only; no tiny wordmark in the tray.
    for path in painter.logo.iter().filter(|p| p.bounds().right() < 30.) {
        pix.fill_path(
            path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::from_translate(1.5, 4.),
            None,
        );
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let Ok(color) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return HICON::default();
    };
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
    for (s, d) in pix
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0.iter_mut())
    {
        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
    let mask_bits = [0u8; 128];
    let mask = CreateBitmap(
        size as i32,
        size as i32,
        1,
        1,
        Some(mask_bits.as_ptr().cast()),
    );
    let icon = CreateIconIndirect(&ICONINFO {
        fIcon: BOOL(1),
        hbmColor: color,
        hbmMask: mask,
        ..Default::default()
    })
    .unwrap_or_default();
    let _ = DeleteObject(color.into());
    let _ = DeleteObject(mask.into());
    if std::env::args().any(|s| s == "--snapshot-preview") {
        let _ = pix.save_png("diagnostics/tray-icon.png");
    }
    icon
}
