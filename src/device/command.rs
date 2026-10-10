//! Bounded command dispatch shared by the UI and device worker.
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;
pub const STATUS_MESSAGE: u32 = WM_APP + 1;
pub const CONTROL_MESSAGE: u32 = WM_APP + 5;
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Mix(u8),
    EqSelect(u8),
    EqRefresh,
    EqLoad(u8),
    EqGain { slot: u8, band: crate::eq::Band },
    Sidetone(bool),
    SidetoneLevel(u8),
    Anc(String),
    Noise(String),
}
impl Command {
    /// Shared UI/transport gate; the transport checks again before writing.
    pub fn supported_by(&self, capabilities: crate::model::SoundCapabilities) -> bool {
        match self {
            Self::Anc(_) => capabilities.anc,
            Self::Sidetone(_) | Self::SidetoneLevel(_) => {
                capabilities.sidetone_max.is_some_and(|max| max > 0)
            }
            Self::Noise(_) => capabilities.noise,
            _ => true,
        }
    }
    /// A band edit replaces only the same band's queued edit, never other bands.
    pub(crate) fn supersedes(&self, old: &Self) -> bool {
        match (self, old) {
            (Self::EqGain { slot: a, band: x }, Self::EqGain { slot: b, band: y }) => {
                a == b && x.id == y.id
            }
            _ => std::mem::discriminant(self) == std::mem::discriminant(old),
        }
    }
}
#[derive(Clone)]
pub(crate) struct QueuedCommand {
    pub sequence: u64,
    pub command: Command,
}
#[derive(Clone)]
pub struct Controller {
    pub(crate) pending: Arc<Mutex<VecDeque<QueuedCommand>>>,
    pub(crate) sequence: Arc<AtomicU64>,
    pub(crate) wake: SyncSender<()>,
    pub(crate) error: Arc<Mutex<Option<String>>>,
}
impl Controller {
    pub fn last_error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
    pub fn clear_error(&self) {
        *self.error.lock().unwrap() = None;
    }

    pub fn send(&self, command: Command) -> u64 {
        let mut queue = self.pending.lock().unwrap();
        // A drag needs the latest position, not a backlog of obsolete positions.
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        queue.retain(|old| !command.supersedes(&old.command));
        queue.push_back(QueuedCommand { sequence, command });
        drop(queue);
        let _ = self.wake.try_send(());
        sequence
    }
}
