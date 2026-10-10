use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};
unsafe extern "system" fn scan(hwnd: HWND, data: LPARAM) -> BOOL {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == data.0 as u32 && IsWindowVisible(hwnd).as_bool() {
        let _ = dump(hwnd);
    }
    BOOL(1)
}
unsafe fn dump(hwnd: HWND) -> Result<()> {
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let root = uia.ElementFromHandle(hwnd)?;
    println!("WINDOW {:?} {}", hwnd, root.CurrentName()?);
    println!("MINIMIZED {}", IsIconic(hwnd).as_bool());
    let all = root.FindAll(TreeScope_Descendants, &uia.CreateTrueCondition()?)?;
    for i in 0..all.Length()? {
        let e = all.GetElement(i)?;
        if e.CurrentName()?.to_string().starts_with("Sidetone")
            || e.CurrentName()?.to_string().starts_with("Game/Chat")
        {
            println!("BOUND {:?}", e.CurrentBoundingRectangle());
        }
        if e.CurrentName()? == "Sidetone" {
            println!(
                "SIDETONE invoke={} default={:?}",
                e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                    .is_ok(),
                e.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                    UIA_LegacyIAccessiblePatternId
                )
                .and_then(|p| p.CurrentDefaultAction())
            );
        }
        let value = e
            .GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId)
            .and_then(|p| {
                Ok((
                    p.CurrentValue()?,
                    p.CurrentMinimum()?,
                    p.CurrentMaximum()?,
                    p.CurrentSmallChange()?,
                ))
            })
            .ok();
        println!(
            "enabled={:?} read_only={:?}",
            e.CurrentIsEnabled(),
            e.GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId)
                .and_then(|p| p.CurrentIsReadOnly())
        );
        println!(
            "{} type={} name={:?} id={:?} value={:?}",
            i,
            e.CurrentControlType()?.0,
            e.CurrentName()?.to_string(),
            e.CurrentAutomationId()?.to_string(),
            value
        );
    }
    Ok(())
}
fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let pid = std::env::args().nth(1).unwrap().parse::<isize>().unwrap();
        EnumWindows(Some(scan), LPARAM(pid))?;
        CoUninitialize();
        Ok(())
    }
}
