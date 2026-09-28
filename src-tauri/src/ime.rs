#[tauri::command]
pub async fn set_japanese_ime(window: tauri::WebviewWindow, enable: bool) {
    #[cfg(windows)]
    if enable {
        if let Ok(hwnd) = window.hwnd() {
            win::enable(hwnd.0);
        }
    } else {
        win::restore();
    }
    #[cfg(target_os = "macos")]
    let _ = window.run_on_main_thread(move || unsafe { hoshi_set_japanese_ime(enable) });
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn hoshi_set_japanese_ime(enable: bool);
}

#[cfg(windows)]
mod win {
    use std::ptr::null_mut;
    use std::sync::Mutex;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP,
        SendInput, VK_IME_ON,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
        PostMessageW, WM_INPUTLANGCHANGEREQUEST,
    };

    static SAVED: Mutex<Option<isize>> = Mutex::new(None);

    fn is_japanese(hkl: HKL) -> bool {
        hkl as usize & 0xFFFF == 0x0411
    }

    unsafe fn foreground_focus() -> HWND {
        unsafe {
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(0, &mut info) != 0 && !info.hwndFocus.is_null() {
                info.hwndFocus
            } else {
                GetForegroundWindow()
            }
        }
    }

    pub fn enable(window: HWND) {
        let mut saved = SAVED.lock().unwrap();
        unsafe {
            if saved.is_some() || GetForegroundWindow() != window {
                return;
            }
            let focus = foreground_focus();
            let thread = GetWindowThreadProcessId(focus, null_mut());
            let previous = GetKeyboardLayout(thread);
            if !is_japanese(previous) {
                let mut layouts = [null_mut(); 32];
                let count = GetKeyboardLayoutList(layouts.len() as i32, layouts.as_mut_ptr());
                let Some(&japanese) = layouts[..count as usize].iter().find(|&&l| is_japanese(l))
                else {
                    return;
                };
                PostMessageW(focus, WM_INPUTLANGCHANGEREQUEST, 0, japanese as isize);
                *saved = Some(previous as isize);
                let switched = (0..20).any(|_| {
                    std::thread::sleep(Duration::from_millis(10));
                    is_japanese(GetKeyboardLayout(thread))
                });
                if !switched {
                    return;
                }
            }
            let mut inputs: [INPUT; 2] = std::mem::zeroed();
            for (input, flags) in inputs.iter_mut().zip([0, KEYEVENTF_KEYUP]) {
                input.r#type = INPUT_KEYBOARD;
                input.Anonymous.ki.wVk = VK_IME_ON;
                input.Anonymous.ki.dwFlags = flags;
            }
            SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32);
        }
    }

    pub fn restore() {
        if let Some(previous) = SAVED.lock().unwrap().take() {
            unsafe { PostMessageW(foreground_focus(), WM_INPUTLANGCHANGEREQUEST, 0, previous) };
        }
    }
}
