//! Per-user launch-at-login entry. No subprocess and no administrator access.
use windows::{
    core::w,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ},
    },
};
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let result = if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command: Vec<u16> = format!("\"{}\"", exe.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: the NUL-terminated UTF-16 buffer lives through the synchronous call.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                w!("AudezeOverlay"),
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
        }
    } else {
        // Deleting a missing value is already the desired state, including on reset.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("AudezeOverlay")) }
    };
    if result == ERROR_SUCCESS
        || (!enabled && (result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND))
    {
        Ok(())
    } else {
        Err(format!(
            "Windows could not update startup (error {}).",
            result.0
        ))
    }
}
