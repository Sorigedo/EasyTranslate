#[path = "../src/platform/hotkeys.rs"]
mod hotkeys;
use easytranslate::shortcuts::Shortcut;
use std::ffi::c_void;
#[repr(C)]
struct Id {
    signature: u32,
    id: u32,
}
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: Id,
        target: *mut c_void,
        options: u32,
        reference: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut c_void) -> i32;
}
fn register(key: Shortcut) -> Result<*mut c_void, i32> {
    let mut reference = std::ptr::null_mut();
    let status = unsafe {
        RegisterEventHotKey(
            key.code,
            key.modifiers,
            Id {
                signature: 1234,
                id: 999,
            },
            GetApplicationEventTarget(),
            0,
            &mut reference,
        )
    };
    if status == 0 {
        Ok(reference)
    } else {
        Err(status)
    }
}
fn main() {
    let mtm = objc2::MainThreadMarker::new().unwrap();
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.finishLaunching();
    let keys = [
        "Control+Option+Shift+F9",
        "Control+Option+Shift+F10",
        "Control+Option+Shift+F11",
    ]
    .map(|s| Shortcut::parse(s).unwrap());
    let mut first = hotkeys::Registry::new().unwrap();
    first.configure(keys).unwrap();
    let occupied = Shortcut::parse("Control+Shift+F8").unwrap();
    let fresh = Shortcut::parse("Control+Shift+F7").unwrap();
    let blocker = register(occupied).unwrap();
    assert!(first.configure([fresh, occupied, keys[2]]).is_err());
    assert!(
        register(keys[0]).is_err(),
        "Failed update released old keys"
    );
    let probe = register(fresh).expect("Failed update leaked partial registrations");
    unsafe {
        UnregisterEventHotKey(probe);
        UnregisterEventHotKey(blocker);
    }
    first
        .configure([keys[1], keys[0], keys[2]])
        .expect("Swapping should preserve registrations");
    assert_eq!(first.take_with_target().0, 0);
    first.suspend();
    let released = register(keys[0]).expect("Recording must release assigned keys");
    assert!(
        first.configure(keys).is_err(),
        "Restore must report a new conflict"
    );
    unsafe {
        UnregisterEventHotKey(released);
    }
    first
        .configure(keys)
        .expect("Recording can restore original bindings");
    assert!(
        register(keys[0]).is_err(),
        "Restored bindings must be registered"
    );
    println!("PASS: real Carbon conflict rollback, partial cleanup and swapping bindings");
}
