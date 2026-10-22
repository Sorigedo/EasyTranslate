use easytranslate::shortcuts::{Shortcut, validate};
use std::{
    ffi::c_void,
    sync::atomic::{AtomicI32, AtomicU32, Ordering},
};
static ACTION: AtomicU32 = AtomicU32::new(0);
static TARGET_PID: AtomicI32 = AtomicI32::new(0);
static LAST_EXTERNAL_PID: AtomicI32 = AtomicI32::new(0);
#[repr(C)]
struct HotKeyID {
    signature: u32,
    id: u32,
}
#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        count: u32,
        types: *const EventType,
        user: *mut c_void,
        output: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyID,
        target: *mut c_void,
        options: u32,
        output: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut c_void) -> i32;
    fn RemoveEventHandler(reference: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual: *mut u32,
        size: u32,
        actual_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}
unsafe extern "C" fn callback(_: *mut c_void, event: *mut c_void, _: *mut c_void) -> i32 {
    let mut id = HotKeyID {
        signature: 0,
        id: 0,
    };
    let result = unsafe {
        GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            std::ptr::null_mut(),
            8,
            std::ptr::null_mut(),
            (&mut id as *mut HotKeyID).cast(),
        )
    };
    if result == 0 {
        let pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
            .unwrap_or(0);
        if pid > 0 && pid != std::process::id() as i32 {
            LAST_EXTERNAL_PID.store(pid, Ordering::Relaxed);
        }
        TARGET_PID.store(pid, Ordering::Relaxed);
        ACTION.store(id.id, Ordering::Relaxed);
    }
    result
}
struct Binding {
    key: Shortcut,
    reference: *mut c_void,
    id: u32,
}
pub struct Registry {
    bindings: Vec<Binding>,
    handler: *mut c_void,
    next_id: u32,
}
impl Registry {
    pub fn new() -> Result<Self, String> {
        let mut handler = std::ptr::null_mut();
        let kind = EventType {
            class: u32::from_be_bytes(*b"keyb"),
            kind: 6,
        };
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                callback,
                1,
                &kind,
                std::ptr::null_mut(),
                &mut handler,
            )
        };
        if status != 0 {
            return Err(format!("全局快捷键监听失败（{status}）"));
        }
        Ok(Self {
            bindings: Vec::new(),
            handler,
            next_id: 1,
        })
    }
    pub fn configure(&mut self, keys: [Shortcut; 3]) -> Result<(), String> {
        validate(&keys)?;
        let mut added: Vec<Binding> = Vec::new();
        for key in keys {
            if self.bindings.iter().any(|b| b.key == key) {
                continue;
            }
            let mut reference = std::ptr::null_mut();
            let id = self.next_id;
            self.next_id += 1;
            let status = unsafe {
                RegisterEventHotKey(
                    key.code,
                    key.modifiers,
                    HotKeyID {
                        signature: u32::from_be_bytes(*b"EZTR"),
                        id,
                    },
                    GetApplicationEventTarget(),
                    0,
                    &mut reference,
                )
            };
            if status != 0 {
                for binding in added {
                    unsafe {
                        UnregisterEventHotKey(binding.reference);
                    }
                }
                return Err(format!(
                    "{} 已被占用或系统拒绝（{status}）；原快捷键保持不变",
                    key.label()
                ));
            }
            added.push(Binding { key, reference, id });
        }
        self.bindings.append(&mut added);
        let mut ordered = Vec::new();
        for key in keys {
            let index = self.bindings.iter().position(|b| b.key == key).unwrap();
            ordered.push(self.bindings.remove(index));
        }
        for binding in self.bindings.drain(..) {
            unsafe {
                UnregisterEventHotKey(binding.reference);
            }
        }
        self.bindings = ordered;
        ACTION.store(0, Ordering::Relaxed);
        TARGET_PID.store(0, Ordering::Relaxed);
        LAST_EXTERNAL_PID.store(0, Ordering::Relaxed);
        Ok(())
    }
    pub fn take_with_target(&self) -> (u32, Option<i32>) {
        let id = ACTION.swap(0, Ordering::Relaxed);
        let recorded_pid = TARGET_PID.swap(0, Ordering::Relaxed);
        let current_frontmost = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
            .filter(|pid| *pid > 0);
        if let Some(pid) = current_frontmost.filter(|pid| *pid != std::process::id() as i32) {
            LAST_EXTERNAL_PID.store(pid, Ordering::Relaxed);
        }
        let action = self
            .bindings
            .iter()
            .position(|b| b.id == id)
            .map(|i| i as u32 + 1)
            .unwrap_or(0);
        let frontmost_pid = (id > 0)
            .then_some(current_frontmost)
            .flatten()
            .filter(|pid| *pid > 0)
            .filter(|pid| *pid != std::process::id() as i32)
            .or_else(|| {
                (recorded_pid > 0 && recorded_pid != std::process::id() as i32)
                    .then_some(recorded_pid)
            })
            .or_else(|| {
                (LAST_EXTERNAL_PID.load(Ordering::Relaxed) > 0)
                    .then(|| LAST_EXTERNAL_PID.load(Ordering::Relaxed))
            });
        (action, frontmost_pid)
    }
    pub fn suspend(&mut self) {
        for binding in self.bindings.drain(..) {
            unsafe {
                UnregisterEventHotKey(binding.reference);
            }
        }
        ACTION.store(0, Ordering::Relaxed);
        TARGET_PID.store(0, Ordering::Relaxed);
        LAST_EXTERNAL_PID.store(0, Ordering::Relaxed);
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        unsafe {
            for binding in &self.bindings {
                UnregisterEventHotKey(binding.reference);
            }
            RemoveEventHandler(self.handler);
        }
    }
}
