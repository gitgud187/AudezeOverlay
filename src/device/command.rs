//! Bounded command dispatch shared by the UI and device worker.
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
};
/// What the worker reports to the application. How a signal reaches the event
/// loop (window message, channel, wake-up) is the application's decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// The shared sound snapshot changed.
    Status,
    /// A queued command finished or failed; see `Controller::last_error`.
    Control,
}
/// Window messages the Windows shell uses to carry a [`Signal`]: WM_APP (0x8000) + n.
#[cfg(windows)]
pub const STATUS_MESSAGE: u32 = 0x8000 + 1;
#[cfg(windows)]
pub const CONTROL_MESSAGE: u32 = 0x8000 + 5;
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
    pub(crate) topology_changed: Arc<AtomicBool>,
    /// The user has turned on Bluetooth control. Off until they do.
    pub(crate) bluetooth_allowed: Arc<AtomicBool>,
    pub(crate) pending: Arc<Mutex<VecDeque<QueuedCommand>>>,
    pub(crate) sequence: Arc<AtomicU64>,
    pub(crate) mix_sequence: Arc<AtomicU64>,
    pub(crate) wake: SyncSender<()>,
    pub(crate) error: Arc<Mutex<Option<String>>>,
}
impl Controller {
    /// Coalesce device-change notifications; enumeration stays on the worker.
    pub fn devices_changed(&self) {
        self.topology_changed.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
    /// Allows or forbids controlling the headset over Bluetooth when no USB
    /// dongle or cable is connected. The worker acts on it at once, closing an
    /// open Bluetooth connection when it is turned off.
    pub fn set_bluetooth_control(&self, allowed: bool) {
        if self.bluetooth_allowed.swap(allowed, Ordering::AcqRel) != allowed {
            self.devices_changed();
        }
    }
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
        if matches!(command, Command::Mix(_)) {
            self.mix_sequence.store(sequence, Ordering::Relaxed);
        }
        queue.retain(|old| !command.supersedes(&old.command));
        queue.push_back(QueuedCommand { sequence, command });
        drop(queue);
        let _ = self.wake.try_send(());
        sequence
    }
    /// Recovery must not supersede a user command queued concurrently.
    pub(crate) fn restore_mix_if_unchanged(&self, expected: u64, chat: u8) {
        let mut queue = self.pending.lock().unwrap();
        if self.mix_sequence.load(Ordering::Relaxed) != expected {
            return;
        }
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        queue.push_back(QueuedCommand {
            sequence,
            command: Command::Mix(chat),
        });
        drop(queue);
        let _ = self.wake.try_send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{sync_channel, Receiver};

    fn controller() -> (Controller, Receiver<()>) {
        let (wake, woken) = sync_channel(1);
        let controller = Controller {
            topology_changed: Arc::new(AtomicBool::new(false)),
            bluetooth_allowed: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(VecDeque::new())),
            sequence: Arc::new(AtomicU64::new(0)),
            mix_sequence: Arc::new(AtomicU64::new(0)),
            wake,
            error: Arc::new(Mutex::new(None)),
        };
        (controller, woken)
    }

    #[test]
    fn bluetooth_control_wakes_the_worker_only_when_the_choice_changes() {
        let (controller, woken) = controller();
        // Saving unrelated settings repeats the current choice; that must stay quiet.
        controller.set_bluetooth_control(false);
        assert!(woken.try_recv().is_err());
        assert!(!controller.topology_changed.load(Ordering::Acquire));

        controller.set_bluetooth_control(true);
        assert!(woken.try_recv().is_ok());
        assert!(controller.topology_changed.swap(false, Ordering::AcqRel));
        assert!(controller.bluetooth_allowed.load(Ordering::Acquire));

        controller.set_bluetooth_control(true);
        assert!(woken.try_recv().is_err());
        assert!(!controller.topology_changed.load(Ordering::Acquire));

        controller.set_bluetooth_control(false);
        assert!(woken.try_recv().is_ok());
        assert!(!controller.bluetooth_allowed.load(Ordering::Acquire));
    }
}
