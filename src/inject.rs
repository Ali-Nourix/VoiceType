use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        },
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

pub fn target() -> Option<isize> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id() {
            None
        } else {
            Some(hwnd as isize)
        }
    }
}
pub fn type_if_same(target: Option<isize>, text: &str) -> Result<(), String> {
    unsafe {
        let expected =
            target.ok_or("No external text field was focused when recording began")? as HWND;
        if GetForegroundWindow() != expected {
            return Err("Focus changed; text was not typed. Copy it below.".into());
        }
        let mut inputs = Vec::new();
        for unit in text.encode_utf16() {
            for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: 0,
                            wScan: unit,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
        }
        if inputs.is_empty() {
            return Ok(());
        }
        let count = SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if count != inputs.len() as u32 {
            Err("Windows blocked text injection. Try Copy instead; elevated apps may require matching permissions.".into())
        } else {
            Ok(())
        }
    }
}
