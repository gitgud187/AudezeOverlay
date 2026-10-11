//! Shortcut recording reads the keyboard state, which is Windows-specific.
use super::*;
pub(super) unsafe fn record(app: &mut App, vk: u32) {
    if !app.recording {
        return;
    }
    if vk == VK_ESCAPE.0 as u32 {
        app.recording = false;
        return;
    }
    if matches!(vk,0x10..=0x12|0x5b..=0x5c|0xa0..=0xa5) {
        return;
    }
    let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
    let mods =
        u32::from(down(VK_MENU)) + 2 * u32::from(down(VK_CONTROL)) + 4 * u32::from(down(VK_SHIFT));
    if !valid_hotkey(mods, vk) || down(VK_LWIN) || down(VK_RWIN) {
        app.note = "Choose one key, or Alt / Ctrl / Shift plus one key. F12 is reserved.".into();
        return;
    }
    app.recorded_key = Some((mods, vk));
    app.note.clear();
}
