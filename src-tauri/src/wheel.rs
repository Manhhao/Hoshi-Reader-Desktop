#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
#[tauri::command]
pub fn set_sideways_wheel(enable: bool) {
    #[cfg(target_os = "macos")]
    mac::SIDEWAYS.store(enable, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
pub mod mac {
    use objc2::encode::{Encoding, RefEncode};
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{msg_send, sel};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[repr(C)]
    struct CGEvent([u8; 0]);

    unsafe impl RefEncode for CGEvent {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateCopy(event: *const CGEvent) -> *mut CGEvent;
        fn CGEventGetIntegerValueField(event: *const CGEvent, field: u32) -> i64;
        fn CGEventSetIntegerValueField(event: *mut CGEvent, field: u32, value: i64);
        fn CGEventGetDoubleValueField(event: *const CGEvent, field: u32) -> f64;
        fn CGEventSetDoubleValueField(event: *mut CGEvent, field: u32, value: f64);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const CGEvent);
    }

    const LINES: (u32, u32) = (11, 12);
    const FIXED: (u32, u32) = (93, 94);
    const POINTS: (u32, u32) = (96, 97);

    type ScrollWheel = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

    static mut ORIGINAL_SCROLL_WHEEL: Option<ScrollWheel> = None;
    pub static SIDEWAYS: AtomicBool = AtomicBool::new(false);

    unsafe extern "C-unwind" fn scroll_wheel(this: *mut AnyObject, sel: Sel, event: *mut AnyObject) {
        unsafe {
            let original = ORIGINAL_SCROLL_WHEEL.unwrap();
            if !SIDEWAYS.load(Ordering::Relaxed) {
                return original(this, sel, event);
            }

            let source: *mut CGEvent = msg_send![event, CGEvent];
            let copy = CGEventCreateCopy(source);
            let lines = CGEventGetIntegerValueField(copy, LINES.1)
                - CGEventGetIntegerValueField(copy, LINES.0);
            let fixed = CGEventGetDoubleValueField(copy, FIXED.1)
                - CGEventGetDoubleValueField(copy, FIXED.0);
            let points = CGEventGetDoubleValueField(copy, POINTS.1)
                - CGEventGetDoubleValueField(copy, POINTS.0);

            CGEventSetIntegerValueField(copy, LINES.0, 0);
            CGEventSetIntegerValueField(copy, LINES.1, lines);
            CGEventSetDoubleValueField(copy, FIXED.0, 0.0);
            CGEventSetDoubleValueField(copy, FIXED.1, fixed);
            CGEventSetDoubleValueField(copy, POINTS.0, 0.0);
            CGEventSetDoubleValueField(copy, POINTS.1, points);

            let sideways: *mut AnyObject =
                msg_send![AnyClass::get(c"NSEvent").unwrap(), eventWithCGEvent: copy];
            CFRelease(copy);
            original(this, sel, sideways);
        }
    }

    pub fn install() {
        unsafe {
            let class = AnyClass::get(c"WKWebView").unwrap();
            let method = class.instance_method(sel!(scrollWheel:)).unwrap();
            let previous =
                method.set_implementation(std::mem::transmute::<ScrollWheel, Imp>(scroll_wheel));
            ORIGINAL_SCROLL_WHEEL = Some(std::mem::transmute::<Imp, ScrollWheel>(previous));
        }
    }
}
