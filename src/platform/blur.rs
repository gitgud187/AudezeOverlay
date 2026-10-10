//! Desktop compositor blur on this window only. No screen capture or shaders
//! are used by the running overlay. Unsupported systems retain transparency.
use windows::{
    core::*,
    Win32::{Foundation::*, System::LibraryLoader::*},
};
#[repr(C)]
struct Accent {
    state: u32,
    flags: u32,
    tint: u32,
    animation: u32,
}
#[repr(C)]
struct Attribute {
    kind: u32,
    data: *mut std::ffi::c_void,
    size: usize,
}
pub unsafe fn apply(hwnd: HWND, enabled: bool) -> bool {
    configure_frame(hwnd, enabled);
    let Ok(module) = GetModuleHandleW(w!("user32.dll")) else {
        return false;
    };
    let Some(address) = GetProcAddress(module, s!("SetWindowCompositionAttribute")) else {
        return false;
    };
    let call: unsafe extern "system" fn(HWND, *mut Attribute) -> BOOL =
        std::mem::transmute(address);
    let mut accent = Accent {
        state: if enabled { 4 } else { 0 },
        flags: 0,
        tint: if enabled { 0x20101010 } else { 0 },
        animation: 0,
    };
    let mut data = Attribute {
        kind: 19,
        data: (&mut accent as *mut Accent).cast(),
        size: std::mem::size_of::<Accent>(),
    };
    call(hwnd, &mut data).as_bool()
}

// A resizable popup still has a DWM non-client surface even when WM_NCPAINT
// is suppressed. Keep that separate rectangular frame out of our rounded UI.
unsafe fn configure_frame(hwnd: HWND, blur: bool) {
    let Ok(module) = LoadLibraryW(w!("dwmapi.dll")) else {
        return;
    };
    if let Some(address) = GetProcAddress(module, s!("DwmSetWindowAttribute")) {
        let call: unsafe extern "system" fn(HWND, u32, *const std::ffi::c_void, u32) -> HRESULT =
            std::mem::transmute(address);
        let disabled = 1u32; // DWMNCRP_DISABLED
        let _ = call(hwnd, 2, (&disabled as *const u32).cast(), 4);
        let no_border = 0xffff_fffeu32; // DWMWA_COLOR_NONE (Windows 11)
        let _ = call(hwnd, 34, (&no_border as *const u32).cast(), 4);
        // HRGN clips our bitmap, but acrylic ignores that mask. Windows 11's
        // ROUND preference clips the backdrop itself to an 8-DIP arc.
        // The raster and region use the same radius. This keeps rectangular
        // blur out of the cutouts; Windows still draws its normal soft shadow.
        let corners = if blur { 2u32 } else { 1u32 };
        let _ = call(hwnd, 33, (&corners as *const u32).cast(), 4);
    }
    let _ = FreeLibrary(module);
}
