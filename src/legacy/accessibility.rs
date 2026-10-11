//! Reads and operates the Audeze Sound page through its accessibility provider.
//! Sends control messages only to Audeze. No game access, process-memory access,
//! device I/O, global input injection, or graphics hooks.
use crate::model::{parse_mix, parse_percent, SoundState};
use std::{
    collections::VecDeque,
    sync::{
        atomic::AtomicU64,
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Variant::VARIANT},
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};

pub use crate::device::command::{Command, Controller, CONTROL_MESSAGE, STATUS_MESSAGE};
#[implement(IUIAutomationPropertyChangedEventHandler)]
struct Changed {
    tx: SyncSender<()>,
}
impl IUIAutomationPropertyChangedEventHandler_Impl for Changed_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        _: Ref<IUIAutomationElement>,
        _: UIA_PROPERTY_ID,
        _: &VARIANT,
    ) -> Result<()> {
        let _ = self.tx.try_send(());
        Ok(())
    }
}
unsafe fn selected(e: &IUIAutomationElement) -> Option<bool> {
    e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
        .and_then(|p| p.CurrentToggleState())
        .map(|v| v == ToggleState_On)
        .ok()
        .or_else(|| {
            e.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
                .and_then(|p| p.CurrentIsSelected())
                .map(|v| v.as_bool())
                .ok()
        })
}
/// # Safety
/// Call on the COM-initialized thread owning these UI Automation interfaces.
pub unsafe fn snapshot(uia: &IUIAutomation, root: &IUIAutomationElement) -> Result<SoundState> {
    let all = root.FindAll(TreeScope_Descendants, &uia.CreateTrueCondition()?)?;
    let mut out = SoundState::default();
    let mut group = 0;
    for i in 0..all.Length()? {
        let e = all.GetElement(i)?;
        let name = e.CurrentName()?.to_string();
        if let Some(mix) = parse_mix(&name) {
            out.mix = Some(mix);
        } else if name == "Sidetone" {
            out.sidetone = selected(&e);
        } else if name.starts_with("Sidetone Level ") {
            out.sidetone_level = parse_percent(&name);
        } else if name == "Active Noise Cancellation (ANC)" {
            group = 1;
        } else if name.contains("AI Noise Removal") {
            group = 2;
        } else if name.starts_with("Battery level is ") {
            out.battery = parse_percent(&name);
        } else if name == "Microphone is muted." {
            out.muted = Some(true);
        } else if name.starts_with("Microphone is ") {
            out.muted = Some(false);
        } else if name.starts_with("Maxwell") {
            out.device = Some(name.clone());
        } else if name == "USB Dongle"
            || name == "USB"
            || name == "USB Cable"
            || name == "Bluetooth"
        {
            out.connection = Some(name.clone());
        } else if selected_if_option(&e, &name) {
            match group {
                1 => out.anc = Some(name.clone()),
                2 => out.noise = Some(name.clone()),
                _ => (),
            }
        }
    }
    out.status = if out.mix.is_some() {
        "Connected to Audeze"
    } else {
        "Open the Sound tab in Audeze"
    }
    .into();
    Ok(out)
}
unsafe fn selected_if_option(e: &IUIAutomationElement, name: &str) -> bool {
    matches!(name, "On" | "Off" | "Transparency" | "Low" | "High") && selected(e) == Some(true)
}
pub fn start(hwnd: isize, shared: Arc<Mutex<SoundState>>) -> Controller {
    let (tx, rx) = sync_channel(1);
    let controller = Controller {
        topology_changed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        sequence: Arc::new(AtomicU64::new(0)),
        mix_sequence: Arc::new(AtomicU64::new(0)),
        pending: Arc::new(Mutex::new(VecDeque::new())),
        wake: tx,
        error: Arc::new(Mutex::new(None)),
    };
    let worker = controller.clone();
    thread::spawn(move || unsafe {
        let result = watch(hwnd, shared.clone(), worker, rx);
        if let Err(e) = result {
            *shared.lock().unwrap() = SoundState {
                status: format!("Audeze connection failed: {e}"),
                ..Default::default()
            };
            let _ = PostMessageW(Some(HWND(hwnd as _)), STATUS_MESSAGE, WPARAM(0), LPARAM(0));
        }
    });
    controller
}
unsafe fn watch(
    hwnd: isize,
    shared: Arc<Mutex<SoundState>>,
    controller: Controller,
    rx: std::sync::mpsc::Receiver<()>,
) -> Result<()> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let handler: IUIAutomationPropertyChangedEventHandler = Changed {
        tx: controller.wake.clone(),
    }
    .into();
    let mut attached: Option<(HWND, IUIAutomationElement)> = None;
    loop {
        // Find only the named companion window. No enumeration or inspection of games.
        let app = FindWindowW(None, w!("Audeze")).ok();
        if app != attached.as_ref().map(|(h, _)| *h) {
            let _ = uia.RemoveAllEventHandlers();
            attached = None;
            if let Some(h) = app {
                if let Ok(root) = uia.ElementFromHandle(h) {
                    uia.AddPropertyChangedEventHandlerNativeArray(
                        &root,
                        TreeScope_Subtree,
                        None,
                        &handler,
                        &[
                            UIA_NamePropertyId,
                            UIA_RangeValueValuePropertyId,
                            UIA_ToggleToggleStatePropertyId,
                            UIA_SelectionItemIsSelectedPropertyId,
                        ],
                    )?;
                    attached = Some((h, root));
                }
            }
        }
        let commands: Vec<_> = controller.pending.lock().unwrap().drain(..).collect();
        let had_commands = !commands.is_empty();
        for command in commands {
            let result = attached
                .as_ref()
                .ok_or_else(|| Error::new(E_FAIL, "Open Audeze on the Sound tab first."))
                .and_then(|(_, root)| apply(&uia, root, &command.command));
            *controller.error.lock().unwrap() = result.err().map(|e| e.message().to_string());
        }
        let state = match attached.as_ref() {
            Some((_, root)) => match snapshot(&uia, root) {
                Ok(s) => s,
                Err(_) => {
                    let _ = uia.RemoveAllEventHandlers();
                    attached = None;
                    SoundState {
                        status: "Reconnecting to Audeze…".into(),
                        ..Default::default()
                    }
                }
            },
            None => SoundState {
                status: "Start Audeze and open Sound".into(),
                ..Default::default()
            },
        };
        let mut latest = shared.lock().unwrap();
        if *latest != state {
            *latest = state;
            drop(latest);
            let _ = PostMessageW(Some(HWND(hwnd as _)), STATUS_MESSAGE, WPARAM(0), LPARAM(0));
        } else {
            drop(latest);
        }
        if had_commands {
            let _ = PostMessageW(Some(HWND(hwnd as _)), CONTROL_MESSAGE, WPARAM(0), LPARAM(0));
        }
        // Event callbacks wake immediately. The timeout repairs missed structural
        // events/restarts and refreshes non-notifying battery text; it isn't a frame loop.
        let _ = rx.recv_timeout(Duration::from_secs(2));
        while rx.try_recv().is_ok() {}
    }
}

/// Only whitelisted Sound controls can be operated; no generic automation API.
/// # Safety
/// The caller must initialize COM and own both interfaces on this thread.
/// `root` must identify the official Audeze Sound page intended to be controlled.
pub unsafe fn apply(
    uia: &IUIAutomation,
    root: &IUIAutomationElement,
    command: &Command,
) -> Result<()> {
    let all = root.FindAll(TreeScope_Descendants, &uia.CreateTrueCondition()?)?;
    let mut group = 0;
    for i in 0..all.Length()? {
        let e = all.GetElement(i)?;
        let name = e.CurrentName()?.to_string();
        if name == "Active Noise Cancellation (ANC)" {
            group = 1;
        }
        if name.contains("AI Noise Removal") {
            group = 2;
        }
        match command {
            Command::Mix(percent) if name.starts_with("Game/Chat Mix ") => {
                return set_range(&e, *percent, true)
            }
            Command::SidetoneLevel(percent) if name.starts_with("Sidetone Level ") => {
                return set_range(&e, *percent, false)
            }
            Command::Sidetone(enabled) if name == "Sidetone" => {
                if selected(&e) != Some(*enabled) {
                    press_audeze_control(&e)?;
                }
                return Ok(());
            }
            Command::Anc(mode)
                if group == 1
                    && name == *mode
                    && matches!(mode.as_str(), "On" | "Off" | "Transparency") =>
            {
                return choose(&e)
            }
            Command::Noise(mode)
                if group == 2
                    && name == *mode
                    && matches!(mode.as_str(), "Off" | "Low" | "High") =>
            {
                return choose(&e)
            }
            _ => (),
        }
    }
    Err(Error::new(
        E_FAIL,
        "That control is unavailable. Open the Sound tab in Audeze.",
    ))
}
unsafe fn set_range(e: &IUIAutomationElement, percent: u8, dial: bool) -> Result<()> {
    if percent > 100 {
        return Err(Error::new(E_INVALIDARG, "Value must be between 0 and 100."));
    }
    let p = e.GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId)?;
    if p.CurrentIsReadOnly()?.as_bool() || !e.CurrentIsEnabled()?.as_bool() {
        return Err(Error::new(E_FAIL, "Enable this control in Audeze first."));
    }
    let (hwnd, r) = control_rect(e)?;
    let min = p.CurrentMinimum()?;
    let max = p.CurrentMaximum()?;
    let current = ((p.CurrentValue()? - min) / (max - min).max(1.)).clamp(0., 1.);
    let at = |fraction: f64| -> POINT {
        if dial {
            let a = (135. + fraction * 270.).to_radians();
            let radius = (r.right - r.left).min(r.bottom - r.top) as f64 * 0.40;
            POINT {
                x: ((r.left + r.right) as f64 / 2. + radius * a.cos()).round() as i32,
                y: ((r.top + r.bottom) as f64 / 2. + radius * a.sin()).round() as i32,
            }
        } else {
            let inset = ((r.bottom - r.top) / 2).min(10);
            POINT {
                x: r.left
                    + inset
                    + ((r.right - r.left - 2 * inset) as f64 * fraction).round() as i32,
                y: (r.top + r.bottom) / 2,
            }
        }
    };
    let from = at(current);
    let to = at(percent as f64 / 100.);
    mouse(hwnd, WM_LBUTTONDOWN, 1, from)?;
    let moved = mouse(hwnd, WM_MOUSEMOVE, 1, to);
    let released = mouse(hwnd, WM_LBUTTONUP, 0, to);
    moved?;
    released
}
unsafe fn choose(e: &IUIAutomationElement) -> Result<()> {
    if selected(e) == Some(true) {
        return Ok(());
    }
    activate(e)
}
unsafe fn activate(e: &IUIAutomationElement) -> Result<()> {
    // Invoke emits the application's clicked action. Merely assigning Checked
    // can change a Qt control's appearance without executing its device command.
    if let Ok(p) = e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
        return p.Invoke();
    }
    e.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId)?
        .DoDefaultAction()
}
/// Qt's switch accessibility action changes Checked without invoking its QML
/// device handler. Deliver a click to that one Audeze control; do not move the
/// system pointer, focus a window, or send anything to another application.
unsafe fn press_audeze_control(e: &IUIAutomationElement) -> Result<()> {
    let (hwnd, r) = control_rect(e)?;
    let point = POINT {
        x: (r.left + r.right) / 2,
        y: (r.top + r.bottom) / 2,
    };
    mouse(hwnd, WM_LBUTTONDOWN, 1, point)?;
    mouse(hwnd, WM_LBUTTONUP, 0, point)
}
unsafe fn control_rect(e: &IUIAutomationElement) -> Result<(HWND, RECT)> {
    let hwnd = FindWindowW(None, w!("Audeze"))?;
    let mut r = e.CurrentBoundingRectangle()?;
    if r.right <= r.left || r.bottom <= r.top {
        return Err(Error::new(E_FAIL, "Restore Audeze to use this control."));
    }
    let mut origin = POINT::default();
    let mut client = RECT::default();
    if IsIconic(hwnd).as_bool() {
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        GetWindowPlacement(hwnd, &mut placement)?;
        origin = POINT {
            x: placement.rcNormalPosition.left,
            y: placement.rcNormalPosition.top,
        };
        client.right = placement.rcNormalPosition.right - origin.x;
        client.bottom = placement.rcNormalPosition.bottom - origin.y;
    } else {
        windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin).ok()?;
        GetClientRect(hwnd, &mut client)?;
    }
    r.left -= origin.x;
    r.right -= origin.x;
    r.top -= origin.y;
    r.bottom -= origin.y;
    if r.left < 0 || r.top < 0 || r.right > client.right || r.bottom > client.bottom {
        return Err(Error::new(
            E_FAIL,
            "Restore Audeze to refresh the control layout.",
        ));
    }
    Ok((hwnd, r))
}
unsafe fn mouse(hwnd: HWND, message: u32, buttons: usize, point: POINT) -> Result<()> {
    let position = LPARAM(((point.x as u32 & 0xffff) | ((point.y as u32 & 0xffff) << 16)) as isize);
    let mut result = 0;
    if SendMessageTimeoutW(
        hwnd,
        message,
        WPARAM(buttons),
        position,
        SMTO_ABORTIFHUNG | SMTO_BLOCK,
        500,
        Some(&mut result),
    )
    .0 == 0
    {
        return Err(Error::from_thread());
    }
    Ok(())
}
