//! Native file pickers. Buffers live for the entire synchronous Windows call.
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{Foundation::HWND, UI::Controls::Dialogs::*},
};

pub fn choose(owner: HWND, save: bool) -> Result<Option<PathBuf>, String> {
    let mut path = [0u16; 32768];
    let initial: Vec<u16> = "My EQ.audeze-eq.json".encode_utf16().collect();
    if save {
        path[..initial.len()].copy_from_slice(&initial);
    }
    let filter: Vec<u16> = if save {
        "Audeze Overlay EQ (*.json)\0*.json\0\0"
    } else {
        "EQ profiles (*.json;*.pnreq)\0*.json;*.pnreq\0\0"
    }
    .encode_utf16()
    .collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(path.as_mut_ptr()),
        nMaxFile: path.len() as u32,
        lpstrDefExt: w!("json"),
        lpstrTitle: if save {
            w!("Export EQ profile")
        } else {
            w!("Import EQ profile")
        },
        Flags: OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    // SAFETY: all pointers reference owned, NUL-terminated buffers above; no hook.
    unsafe {
        let accepted = if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        };
        if !accepted.as_bool() {
            let error = CommDlgExtendedError().0;
            return if error == 0 {
                Ok(None)
            } else {
                Err(format!("File picker failed ({error:#x})."))
            };
        }
    }
    let length = path
        .iter()
        .position(|ch| *ch == 0)
        .ok_or("Invalid file picker path.")?;
    Ok(Some(PathBuf::from(OsString::from_wide(&path[..length]))))
}
