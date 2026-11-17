#[cfg(windows)]
pub fn type_text(text: &str, delay_ms: u64) {
    use std::thread::sleep;
    use std::time::Duration;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    let utf16_chars: Vec<u16> = text.encode_utf16().collect();
    let delay = Duration::from_millis(delay_ms);

    for ch in utf16_chars {
        // Prepare keydown
        let input_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        // Prepare keyup
        let input_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        unsafe {
            let mut inputs = [input_down, input_up];
            SendInput(
                2,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }

        if delay_ms > 0 {
            sleep(delay);
        }
    }
}

#[cfg(not(windows))]
pub fn type_text(_text: &str, _delay_ms: u64) {
    eprintln!("Auto-typing is only implemented for Windows in this build.");
}
