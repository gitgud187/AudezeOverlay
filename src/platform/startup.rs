//! Per-user launch-at-login entry. No subprocess and no administrator access.
use std::path::{Path, PathBuf};
use windows::{
    core::w,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
            RRF_RT_REG_SZ,
        },
    },
};

fn startup_command(current: &Path, installed: Option<&Path>) -> String {
    format!("\"{}\"", installed.unwrap_or(current).display())
}

fn desired_command() -> Result<String, String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    // Portable/development copies share preferences with the installed app.
    // Do not let a temporary test executable take over its login entry.
    let installed = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| {
            p.join("Programs")
                .join("Audeze Overlay")
                .join("Audeze Overlay.exe")
        })
        .filter(|p| p.is_file());
    Ok(startup_command(&current, installed.as_deref()))
}

fn registered_command() -> Result<Option<String>, String> {
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = w!("AudezeOverlay");
    let mut bytes = 0;
    // SAFETY: predefined root key and static strings; the size output is valid.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS || bytes == 0 || bytes > 65536 || bytes % 2 != 0 {
        return Err(format!(
            "Windows could not read startup (error {}).",
            result.0
        ));
    }
    let mut value = vec![0u16; bytes as usize / 2];
    // SAFETY: value has the requested byte capacity and lives through the call.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(format!(
            "Windows could not read startup (error {}).",
            result.0
        ));
    }
    let end = value.iter().position(|c| *c == 0).unwrap_or(value.len());
    Ok(Some(String::from_utf16_lossy(&value[..end])))
}

/// Repair an enabled login entry once at normal startup. No periodic registry work.
/// Windows' separate Task Manager startup-disable setting is not modified.
pub fn reconcile(enabled: bool) -> Result<(), String> {
    if enabled && registered_command()?.as_deref() != Some(desired_command()?.as_str()) {
        set_enabled(true)?;
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let result = if enabled {
        let command: Vec<u16> = desired_command()?.encode_utf16().chain(Some(0)).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_copy_takes_precedence_over_temporary_builds() {
        let installed =
            Path::new(r"C:\Users\Tim\AppData\Local\Programs\Audeze Overlay\Audeze Overlay.exe");
        for current in [
            Path::new(r"C:\Dev\dist\Audeze Overlay.exe"),
            Path::new(r"C:\Dev\target\release\audeze-overlay.exe"),
            installed,
        ] {
            assert_eq!(
                startup_command(current, Some(installed)),
                format!("\"{}\"", installed.display())
            );
        }
    }

    #[test]
    fn portable_only_startup_quotes_the_actual_path() {
        let portable = Path::new(r"C:\My Apps\Audeze Overlay.exe");
        assert_eq!(
            startup_command(portable, None),
            r#""C:\My Apps\Audeze Overlay.exe""#
        );
    }
}
