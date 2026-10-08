#[tauri::command]
pub fn clipboard_text(since: Option<i64>) -> (i64, Option<String>) {
    let count = change_count();
    let text = if since.is_some_and(|since| since != count) {
        text()
    } else {
        None
    };
    (count, text)
}

#[cfg(target_os = "macos")]
fn pasteboard() -> *mut objc2::runtime::AnyObject {
    use objc2::runtime::AnyClass;
    unsafe { objc2::msg_send![AnyClass::get(c"NSPasteboard").unwrap(), generalPasteboard] }
}

#[cfg(target_os = "macos")]
fn change_count() -> i64 {
    let count: isize = unsafe { objc2::msg_send![pasteboard(), changeCount] };
    count as i64
}

#[cfg(target_os = "macos")]
fn string_for_type(kind: &std::ffi::CStr) -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::{CStr, c_char};
    unsafe {
        let kind: *mut AnyObject = msg_send![
            AnyClass::get(c"NSString").unwrap(),
            stringWithUTF8String: kind.as_ptr()
        ];
        let string: *mut AnyObject = msg_send![pasteboard(), stringForType: kind];
        if string.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![string, UTF8String];
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

#[cfg(target_os = "macos")]
fn text() -> Option<String> {
    if string_for_type(c"public.file-url").is_some() {
        return None;
    }
    string_for_type(c"public.utf8-plain-text")
}

#[cfg(windows)]
fn change_count() -> i64 {
    unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() as i64 }
}

#[cfg(windows)]
fn text() -> Option<String> {
    use std::ptr::null_mut;
    use std::time::Duration;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;

    unsafe {
        let opened = (0..5).any(|_| {
            OpenClipboard(null_mut()) != 0 || {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        if !opened {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        let chars = GlobalLock(handle) as *const u16;
        let text = (!chars.is_null()).then(|| {
            let len = (0..).take_while(|&i| *chars.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(chars, len))
        });
        GlobalUnlock(handle);
        CloseClipboard();
        text
    }
}
