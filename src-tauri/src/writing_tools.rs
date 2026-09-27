use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel};

type Init = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject;

static mut ORIGINAL_INIT: Option<Init> = None;

unsafe extern "C-unwind" fn init(this: *mut AnyObject, sel: Sel) -> *mut AnyObject {
    unsafe {
        let config = ORIGINAL_INIT.unwrap()(this, sel);
        let _: () = msg_send![config, setWritingToolsBehavior: -1isize];
        config
    }
}

pub fn disable() {
    unsafe {
        let class = AnyClass::get(c"WKWebViewConfiguration").unwrap();
        let method = class.instance_method(sel!(init)).unwrap();
        let previous = method.set_implementation(std::mem::transmute::<Init, Imp>(init));
        ORIGINAL_INIT = Some(std::mem::transmute::<Imp, Init>(previous));
    }
}
