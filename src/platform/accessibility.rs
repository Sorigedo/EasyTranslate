use easytranslate::placement::Rect;
use std::ffi::c_void;
use std::ptr;

type Ref = *const c_void;
#[repr(C)]
struct CFRange {
    location: isize,
    length: isize,
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    fn AXUIElementIsAttributeSettable(element: Ref, attribute: Ref, value: *mut bool) -> i32;
    fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
    fn AXUIElementGetPid(element: Ref, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Ref, timeout: f32) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetType(value: Ref) -> u32;
    fn AXValueGetValue(value: Ref, kind: u32, result: *mut c_void) -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFRetain(value: Ref) -> Ref;
    fn CFEqual(a: Ref, b: Ref) -> bool;
    fn CFHash(value: Ref) -> usize;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Ref) -> bool;
    fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        count: isize,
        encoding: u32,
        external: bool,
    ) -> Ref;
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetBytes(
        value: Ref,
        range: CFRange,
        encoding: u32,
        loss_byte: u8,
        external: bool,
        buffer: *mut u8,
        size: isize,
        used: *mut isize,
    ) -> isize;
}
const UTF8: u32 = 0x08000100;
#[repr(C)]
struct Pair {
    x: f64,
    y: f64,
}
pub struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
impl Clone for Owned {
    fn clone(&self) -> Self {
        Self(unsafe { CFRetain(self.0) })
    }
}
impl Owned {
    fn string(text: &str) -> Self {
        Self(unsafe {
            CFStringCreateWithBytes(ptr::null(), text.as_ptr(), text.len() as isize, UTF8, false)
        })
    }
    fn attribute(&self, name: &str) -> Option<Self> {
        let name = Self::string(name);
        let mut value = ptr::null();
        (unsafe { AXUIElementCopyAttributeValue(self.0, name.0, &mut value) } == 0
            && !value.is_null())
        .then(|| Self(value))
    }
    fn text(&self) -> Option<String> {
        unsafe {
            if CFGetTypeID(self.0) != CFStringGetTypeID() {
                return None;
            }
            let length = CFStringGetLength(self.0);
            let capacity = CFStringGetMaximumSizeForEncoding(length, UTF8).max(1);
            let mut bytes = vec![0u8; capacity as usize];
            let mut used = 0;
            let converted = CFStringGetBytes(
                self.0,
                CFRange {
                    location: 0,
                    length,
                },
                UTF8,
                0,
                false,
                bytes.as_mut_ptr(),
                capacity,
                &mut used,
            );
            if converted != length {
                return None;
            }
            bytes.truncate(used as usize);
            String::from_utf8(bytes).ok()
        }
    }
    fn string_attribute(&self, name: &str) -> Option<String> {
        self.attribute(name)?.text()
    }
    fn bool_attribute(&self, name: &str) -> bool {
        let Some(value) = self.attribute(name) else {
            return false;
        };
        unsafe { CFGetTypeID(value.0) == CFBooleanGetTypeID() && CFBooleanGetValue(value.0) }
    }
    fn children(&self) -> Vec<Self> {
        let Some(children) = self.attribute("AXChildren") else {
            return Vec::new();
        };
        if unsafe { CFGetTypeID(children.0) != CFArrayGetTypeID() } {
            return Vec::new();
        }
        let count = unsafe { CFArrayGetCount(children.0) };
        (0..count)
            .filter_map(|index| {
                let value = unsafe { CFArrayGetValueAtIndex(children.0, index) };
                (!value.is_null() && unsafe { CFGetTypeID(value) == AXUIElementGetTypeID() })
                    .then(|| Self(unsafe { CFRetain(value) }))
            })
            .collect()
    }
    fn settable(&self, name: &str) -> bool {
        let name = Self::string(name);
        let mut result = false;
        unsafe { AXUIElementIsAttributeSettable(self.0, name.0, &mut result) == 0 && result }
    }
    fn set(&self, name: &str, value: &Self) -> bool {
        unsafe { AXUIElementSetAttributeValue(self.0, Self::string(name).0, value.0) == 0 }
    }
    fn pair_attribute(&self, name: &str, kind: u32) -> Option<Pair> {
        let value = self.attribute(name)?;
        let mut pair = Pair { x: 0., y: 0. };
        (unsafe {
            CFGetTypeID(value.0) == AXValueGetTypeID()
                && AXValueGetType(value.0) == kind
                && AXValueGetValue(value.0, kind, (&mut pair as *mut Pair).cast())
        })
        .then_some(pair)
    }
    fn frame(&self) -> Option<Rect> {
        let position = self.pair_attribute("AXPosition", 1)?;
        let size = self.pair_attribute("AXSize", 2)?;
        (position.x.is_finite()
            && position.y.is_finite()
            && size.x.is_finite()
            && size.y.is_finite()
            && size.x > 0.
            && size.y > 0.)
            .then(|| Rect::new(position.x, position.y, size.x, size.y))
    }
}
#[derive(Clone)]
pub struct Focus {
    element: Owned,
    window: Option<Owned>,
    pub pid: i32,
    pub text: String,
    pub token: u64,
    pub frame: Option<Rect>,
}
pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}
fn pick_editor(candidates: &[(bool, bool)]) -> Option<usize> {
    let focused: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, (editable, focused))| (*editable && *focused).then_some(index))
        .collect();
    if focused.len() == 1 {
        return focused.first().copied();
    }
    if focused.len() > 1 {
        return None;
    }
    let editable: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, (editable, _))| editable.then_some(index))
        .collect();
    (editable.len() == 1).then(|| editable[0])
}
fn editor_in_window(window: &Owned) -> Option<Owned> {
    if unsafe { CFGetTypeID(window.0) != AXUIElementGetTypeID() } {
        return None;
    }
    let mut stack = vec![(window.clone(), 0usize)];
    let mut candidates = Vec::new();
    let mut visited = 0;
    while let Some((element, depth)) = stack.pop() {
        visited += 1;
        if visited > 300 {
            return None;
        }
        if depth < 14 {
            stack.extend(
                element
                    .children()
                    .into_iter()
                    .map(|child| (child, depth + 1)),
            );
        }
        let role = element.string_attribute("AXRole").unwrap_or_default();
        if matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox")
            && element.string_attribute("AXSubrole").as_deref() != Some("AXSecureTextField")
        {
            let editable = element.settable("AXValue") || element.settable("AXSelectedText");
            candidates.push((element, editable));
        }
    }
    let index = pick_editor(
        &candidates
            .iter()
            .map(|(element, editable)| (*editable, element.bool_attribute("AXFocused")))
            .collect::<Vec<_>>(),
    )?;
    Some(candidates.swap_remove(index).0)
}
fn editor_in_focused_window(app: &Owned) -> Option<Owned> {
    let window = app.attribute("AXFocusedWindow")?;
    editor_in_window(&window)
}
fn editor_in_app(app: &Owned) -> Option<Owned> {
    let windows = app.attribute("AXWindows")?;
    if unsafe { CFGetTypeID(windows.0) != CFArrayGetTypeID() } {
        return None;
    }
    let count = unsafe { CFArrayGetCount(windows.0) };
    let editors: Vec<_> = (0..count)
        .filter_map(|index| {
            let value = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
            (!value.is_null() && unsafe { CFGetTypeID(value) == AXUIElementGetTypeID() })
                .then(|| Owned(unsafe { CFRetain(value) }))
        })
        .filter_map(|window| editor_in_window(&window))
        .collect();
    (editors.len() == 1).then(|| editors.into_iter().next().unwrap())
}
fn selected_text_in_tree(root: &Owned) -> Option<String> {
    let mut stack = vec![(root.clone(), 0usize)];
    let mut visited = 0;
    while let Some((element, depth)) = stack.pop() {
        visited += 1;
        if visited > 500 {
            return None;
        }
        if element.string_attribute("AXSubrole").as_deref() != Some("AXSecureTextField")
            && let Some(value) = element
                .string_attribute("AXSelectedText")
                .filter(|value| !value.trim().is_empty())
        {
            return Some(value);
        }
        if depth < 16 {
            stack.extend(
                element
                    .children()
                    .into_iter()
                    .map(|child| (child, depth + 1)),
            );
        }
    }
    None
}
fn selected_text_in_app(app: &Owned) -> Option<String> {
    if let Some(window) = app.attribute("AXFocusedWindow")
        && let Some(value) = selected_text_in_tree(&window)
    {
        return Some(value);
    }
    let windows = app.attribute("AXWindows")?;
    if unsafe { CFGetTypeID(windows.0) != CFArrayGetTypeID() } {
        return None;
    }
    let count = unsafe { CFArrayGetCount(windows.0) };
    (0..count).find_map(|index| {
        let value = unsafe { CFArrayGetValueAtIndex(windows.0, index) };
        (!value.is_null() && unsafe { CFGetTypeID(value) == AXUIElementGetTypeID() })
            .then(|| Owned(unsafe { CFRetain(value) }))
            .and_then(|window| selected_text_in_tree(&window))
    })
}
fn focused_element_in_pid(target_pid: i32) -> Result<Owned, String> {
    if !trusted() {
        return Err(
            "当前版本未获辅助功能授权。若开关已开，请移除旧条目后重新添加当前 EasyTranslate.app。"
                .into(),
        );
    }
    let app = Owned(unsafe { AXUIElementCreateApplication(target_pid) });
    unsafe {
        AXUIElementSetMessagingTimeout(app.0, 0.15);
    }
    let element = app
        .attribute("AXFocusedUIElement")
        .or_else(|| editor_in_focused_window(&app))
        .ok_or("当前窗口未提供唯一可编辑输入框；请点回聊天输入框再按快捷键")?;
    unsafe {
        AXUIElementSetMessagingTimeout(element.0, 0.15);
    }
    Ok(element)
}
pub fn focused_in_pid(target_pid: i32) -> Result<Focus, String> {
    let mut element = focused_element_in_pid(target_pid)?;
    if element.string_attribute("AXSubrole").as_deref() == Some("AXSecureTextField") {
        return Err("不读取密码框".into());
    }
    if !matches!(
        element.string_attribute("AXRole").as_deref(),
        Some("AXTextField" | "AXTextArea" | "AXComboBox")
    ) {
        let app = Owned(unsafe { AXUIElementCreateApplication(target_pid) });
        element = editor_in_focused_window(&app)
            .ok_or("当前窗口未提供唯一可编辑输入框；请点回聊天输入框再按快捷键")?;
    }
    let role = element.string_attribute("AXRole").unwrap_or_default();
    let subrole = element.string_attribute("AXSubrole").unwrap_or_default();
    if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox")
        || subrole == "AXSecureTextField"
    {
        return Err("当前焦点不是可读取的文本输入框".into());
    }
    if !element.settable("AXValue") && !element.settable("AXSelectedText") {
        return Err("此输入框未提供可验证的替换接口，请使用手动翻译。".into());
    }
    let text = element
        .string_attribute("AXValue")
        .ok_or("无法读取输入框草稿")?;
    let mut pid = 0;
    unsafe {
        AXUIElementGetPid(element.0, &mut pid);
    }
    if pid != target_pid {
        return Err("输入焦点已切换到其他应用，请重新按快捷键".into());
    }
    let token = (unsafe { CFHash(element.0) } as u64) ^ ((pid as u64) << 32);
    let window = element.attribute("AXWindow").or_else(|| {
        let app = Owned(unsafe { AXUIElementCreateApplication(target_pid) });
        app.attribute("AXFocusedWindow")
    });
    Ok(Focus {
        frame: element.frame(),
        element,
        window,
        pid,
        text,
        token,
    })
}
pub fn focused() -> Result<Focus, String> {
    let pid = frontmost_pid().ok_or("无法确定当前应用")?;
    focused_in_pid(pid)
}
pub fn selection() -> Result<String, String> {
    let pid = frontmost_pid().ok_or("无法确定当前应用")?;
    selection_in_pid(pid)
}
pub fn selection_in_pid(pid: i32) -> Result<String, String> {
    let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
    unsafe {
        AXUIElementSetMessagingTimeout(app.0, 0.15);
    }
    if !trusted() {
        return Err(
            "当前版本未获辅助功能授权。若开关已开，请移除旧条目后重新添加当前 EasyTranslate.app。"
                .into(),
        );
    }
    if let Ok(element) = focused_element_in_pid(pid) {
        if element.string_attribute("AXSubrole").as_deref() == Some("AXSecureTextField") {
            return Err("不读取密码框".into());
        }
        if let Some(value) = element
            .string_attribute("AXSelectedText")
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(value);
        }
    }
    selected_text_in_app(&app).ok_or("应用未提供选中文字；请先选中文字后再按快捷键。".into())
}
pub fn same_target(a: &Focus, b: &Focus) -> bool {
    a.pid == b.pid
        && unsafe { CFEqual(a.element.0, b.element.0) }
        && match (&a.window, &b.window) {
            (Some(a), Some(b)) => unsafe { CFEqual(a.0, b.0) },
            _ => false,
        }
}
pub fn replace(expected: &Focus, translated: &str) -> Result<(), String> {
    let current = focused()?;
    if !same_target(expected, &current)
        || current.text != expected.text
        || translated.trim().is_empty()
    {
        return Err("输入框或草稿已变化，请等待最新译文。".into());
    }
    let value = Owned::string(translated);
    if current.element.settable("AXValue") {
        if !current.element.set("AXValue", &value) {
            return Err("应用拒绝替换草稿，请复制译文。".into());
        }
    } else {
        #[repr(C)]
        struct Range {
            location: isize,
            length: isize,
        }
        let range = Range {
            location: 0,
            length: current.text.encode_utf16().count() as isize,
        };
        let pointer = unsafe { AXValueCreate(4, (&range as *const Range).cast()) };
        if pointer.is_null() {
            return Err("无法创建文本选区".into());
        }
        let range_value = Owned(pointer);
        if !current.element.set("AXSelectedTextRange", &range_value) {
            return Err("应用不允许设置选区，请复制译文。".into());
        }
        let verified = focused()?;
        if !same_target(&current, &verified)
            || verified.text != expected.text
            || verified
                .element
                .string_attribute("AXSelectedText")
                .as_deref()
                != Some(expected.text.as_str())
        {
            return Err("无法验证待替换选区，已停止。".into());
        }
        if !verified.element.set("AXSelectedText", &value) {
            return Err("替换失败，请复制译文。".into());
        }
    }
    if current.element.string_attribute("AXValue").as_deref() != Some(translated) {
        return Err("应用未确认替换结果，请检查草稿；不会自动重试。".into());
    }
    Ok(())
}

pub fn frontmost_pid() -> Option<i32> {
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| a.processIdentifier())
}

pub fn target_diagnostic(f: &Focus) -> String {
    format!(
        "pid={} element={} window={:?}",
        f.pid,
        unsafe { CFHash(f.element.0) },
        f.window.as_ref().map(|w| unsafe { CFHash(w.0) })
    )
}

#[cfg(test)]
mod tests {
    use super::{Owned, pick_editor};

    #[test]
    fn native_text_preserves_entire_draft_including_embedded_null() {
        for value in ["你好😀\n第二行", "不要付款\0等待确认", "\0", ""] {
            assert_eq!(Owned::string(value).text().as_deref(), Some(value));
        }
    }

    #[test]
    fn focused_editor_wins_only_among_editable_candidates() {
        assert_eq!(pick_editor(&[(true, false), (true, true)]), Some(1));
        assert_eq!(pick_editor(&[(false, true), (true, false)]), Some(1));
    }

    #[test]
    fn ambiguous_editors_are_never_selected() {
        assert_eq!(pick_editor(&[(true, false), (true, false)]), None);
        assert_eq!(pick_editor(&[(true, true), (true, true)]), None);
        assert_eq!(pick_editor(&[(false, true)]), None);
    }
}
