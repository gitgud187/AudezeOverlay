//! Opt-in integration check: briefly change mix, read it back, then restore it.
use audeze_overlay::{
    legacy::accessibility::{self as telemetry, apply, snapshot, Command},
    model::SoundState,
};
use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        System::Com::*,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};
pub fn main() -> Result<()> {
    unsafe {
        let all_controls = std::env::args().any(|a| a == "--all-and-restore");
        if !all_controls && !std::env::args().any(|a| a == "--mix-and-restore") {
            println!("Use --mix-and-restore or --all-and-restore for reversible live checks.");
            return Ok(());
        }
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let hwnd = FindWindowW(None, w!("Audeze"))?;
        let minimized = IsIconic(hwnd).as_bool();
        println!("Audeze minimized={minimized}");
        struct Restore(windows::Win32::Foundation::HWND, bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                if self.1 {
                    unsafe {
                        let _ = ShowWindow(self.0, SW_MINIMIZE);
                    }
                }
            }
        }
        let keep_minimized = std::env::args().any(|a| a == "--keep-minimized");
        let _restore = Restore(hwnd, minimized && !keep_minimized);
        if minimized && !keep_minimized {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            sleep(Duration::from_millis(250));
        }
        let root = uia.ElementFromHandle(hwnd)?;
        let before = snapshot(&uia, &root)?;
        let observed = Arc::new(Mutex::new(SoundState::default()));
        let _listener = telemetry::start(0, observed.clone());
        let ready = Instant::now();
        while observed.lock().unwrap().mix.is_none() && ready.elapsed() < Duration::from_secs(2) {
            sleep(Duration::from_millis(5));
        }
        println!("before {}", serde_json::to_string(&before).unwrap());
        if let Some((_, old)) = before.mix {
            let target = if old < 100 { old + 5 } else { old - 5 };
            let started = Instant::now();
            let attempted = apply(&uia, &root, &Command::Mix(target));
            while observed.lock().unwrap().mix == before.mix
                && started.elapsed() < Duration::from_secs(3)
            {
                sleep(Duration::from_millis(2));
            }
            println!(
                "listener observed mix after {} ms",
                started.elapsed().as_millis()
            );
            std::thread::sleep(std::time::Duration::from_millis(350));
            let changed = snapshot(&uia, &root);
            let restored = apply(&uia, &root, &Command::Mix(old));
            std::thread::sleep(std::time::Duration::from_millis(350));
            println!("write={attempted:?}; changed={changed:?}; restore={restored:?}");
            println!("after {:?}", snapshot(&uia, &root));
            attempted?;
            restored?;
            assert_eq!(
                changed?.mix,
                Some((100 - target, target)),
                "Native mix command did not apply"
            );
            assert_eq!(
                snapshot(&uia, &root)?.mix,
                before.mix,
                "Mix did not restore"
            );
        }
        if all_controls {
            if let Some(ref anc) = before.anc {
                check_pair(
                    &uia,
                    &root,
                    Command::Anc(if anc == "Off" { "Transparency" } else { "Off" }.into()),
                    Command::Anc(anc.clone()),
                )?;
            }
            if let Some(ref noise) = before.noise {
                check_pair(
                    &uia,
                    &root,
                    Command::Noise(if noise == "Off" { "Low" } else { "Off" }.into()),
                    Command::Noise(noise.clone()),
                )?;
            }
            if let (Some(side), Some(level)) = (before.sidetone, before.sidetone_level) {
                let enabled = apply(&uia, &root, &Command::Sidetone(true));
                sleep(Duration::from_millis(700));
                let result = if enabled.is_ok() {
                    check_pair(
                        &uia,
                        &root,
                        Command::SidetoneLevel((level + 4).min(100)),
                        Command::SidetoneLevel(level),
                    )
                } else {
                    enabled
                };
                let restored = apply(&uia, &root, &Command::Sidetone(side));
                sleep(Duration::from_millis(150));
                println!("sidetone restored: {:?}", snapshot(&uia, &root));
                result?;
                restored?;
            }
        }
        // Same-value radio selections exercise control lookup without changing modes.
        if let Some(anc) = before.anc {
            println!("ANC selected: {:?}", apply(&uia, &root, &Command::Anc(anc)));
        }
        if let Some(noise) = before.noise {
            println!(
                "FILTER selected: {:?}",
                apply(&uia, &root, &Command::Noise(noise))
            );
        }
        println!("Audeze remains minimized={}", IsIconic(hwnd).as_bool());
        CoUninitialize();
        Ok(())
    }
}
unsafe fn check_pair(
    uia: &IUIAutomation,
    root: &IUIAutomationElement,
    set: Command,
    restore: Command,
) -> Result<()> {
    let changed = apply(uia, root, &set);
    sleep(Duration::from_millis(180));
    let value = snapshot(uia, root);
    let restored = apply(uia, root, &restore);
    sleep(Duration::from_millis(180));
    let after = snapshot(uia, root);
    println!("set {set:?}: {changed:?} read={value:?}; restored={restored:?} after={after:?}");
    changed?;
    restored?;
    Ok(())
}
