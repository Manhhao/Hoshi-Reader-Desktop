#[tauri::command]
pub fn set_menu_theme(dark: Option<bool>) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

        let uxtheme = LoadLibraryW(windows_sys::w!("uxtheme.dll"));
        if uxtheme.is_null() {
            return;
        }
        let mode = match dark {
            None => 1,
            Some(true) => 2,
            Some(false) => 3,
        };
        if let Some(set_preferred_app_mode) = GetProcAddress(uxtheme, 135 as _) {
            let set_preferred_app_mode: unsafe extern "system" fn(i32) -> i32 =
                std::mem::transmute(set_preferred_app_mode);
            set_preferred_app_mode(mode);
        }
        if let Some(flush_menu_themes) = GetProcAddress(uxtheme, 136 as _) {
            let flush_menu_themes: unsafe extern "system" fn() =
                std::mem::transmute(flush_menu_themes);
            flush_menu_themes();
        }
    }
    #[cfg(not(windows))]
    let _ = dark;
}
