//! One outstanding HID input read. Windows sleeps until a report or timeout.
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Threading::*, IO::*},
    },
};

pub struct InputListener {
    handle: HANDLE,
    event: HANDLE,
    // Both allocations must remain at stable addresses until I/O completes.
    overlapped: Box<OVERLAPPED>,
    buffer: Box<[u8]>,
    pending: bool,
}
impl InputListener {
    pub(crate) fn open(path: &[u16], len: usize) -> Result<Self, String> {
        unsafe {
            let handle = CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(|e| e.to_string())?;
            let event = match CreateEventW(None, true, false, None) {
                Ok(h) => h,
                Err(e) => {
                    let _ = CloseHandle(handle);
                    return Err(e.to_string());
                }
            };
            Ok(Self {
                handle,
                event,
                overlapped: Box::new(OVERLAPPED {
                    hEvent: event,
                    ..Default::default()
                }),
                buffer: vec![0; len].into_boxed_slice(),
                pending: false,
            })
        }
    }
    pub fn read(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        unsafe {
            if !self.pending {
                ResetEvent(self.event).map_err(|e| e.to_string())?;
                *self.overlapped = OVERLAPPED {
                    hEvent: self.event,
                    ..Default::default()
                };
                if let Err(e) = ReadFile(
                    self.handle,
                    Some(&mut self.buffer),
                    None,
                    Some(&mut *self.overlapped),
                ) {
                    if e.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0) {
                        return Err(e.to_string());
                    }
                }
                self.pending = true;
            }
            match WaitForSingleObject(self.event, timeout_ms) {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => {
                    let mut count = 0;
                    let result =
                        GetOverlappedResult(self.handle, &*self.overlapped, &mut count, false);
                    self.pending = false;
                    result.map_err(|e| e.to_string())?;
                    Ok(Some(self.buffer[..count as usize].to_vec()))
                }
                _ => Err(windows::core::Error::from_thread().to_string()),
            }
        }
    }
}
impl Drop for InputListener {
    fn drop(&mut self) {
        unsafe {
            if self.pending {
                let _ = CancelIoEx(self.handle, Some(&*self.overlapped));
                let mut count = 0;
                let _ = GetOverlappedResult(self.handle, &*self.overlapped, &mut count, true);
            }
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.event);
        }
    }
}
