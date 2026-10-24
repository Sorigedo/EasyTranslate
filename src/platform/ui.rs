use super::{accessibility as ax, hotkeys};
use easytranslate::shortcuts::{Config, Shortcut};
use easytranslate::{
    bridge::{self, Request},
    cache::Cache,
    draft::{Draft, Snapshot},
    operation::Operation,
    placement::{Rect, popup_origin},
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::*;
use objc2_core_graphics::{CGEvent, CGEventFlags};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer};
use std::{
    cell::RefCell,
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn toggle_action(panel_visible: bool, app_frontmost: bool, panel_key: bool) -> bool {
    // A visible panel can still be behind another application. Treat that
    // state as an "open" request so the global shortcut raises it first;
    // only a panel that is both visible and key should be closed.
    !panel_visible || !app_frontmost || !panel_key
}

fn selected_text_copy_first(
    copy: impl FnOnce() -> Result<String, String>,
    accessibility: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    match copy() {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => accessibility(),
    }
}

fn shortcut_menu_title(title: &str, shortcut: Shortcut) -> String {
    let key = shortcut
        .label()
        .replace("Control+", "⌃")
        .replace("Option+", "⌥")
        .replace("Shift+", "⇧")
        .replace("Command+", "⌘");
    format!("{title}（{key}）")
}

fn copy_shortcut_to_pid(pid: i32) -> Result<String, String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let previous_change_count = pasteboard.changeCount();
    let key_down =
        CGEvent::new_keyboard_event(None, 8, true).ok_or("无法创建 Command+C 复制事件")?;
    CGEvent::set_flags(Some(&key_down), CGEventFlags::MaskCommand);
    CGEvent::post_to_pid(pid, Some(&key_down));
    let key_up =
        CGEvent::new_keyboard_event(None, 8, false).ok_or("无法创建 Command+C 复制事件")?;
    CGEvent::set_flags(Some(&key_up), CGEventFlags::MaskCommand);
    CGEvent::post_to_pid(pid, Some(&key_up));

    let deadline = Instant::now() + Duration::from_millis(450);
    while Instant::now() < deadline {
        if pasteboard.changeCount() != previous_change_count
            && let Some(value) = pasteboard
                .stringForType(&text("public.utf8-plain-text"))
                .map(|value| value.to_string())
                .filter(|value| !value.trim().is_empty())
        {
            return Ok(value);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("没有复制到选中文字".into())
}

fn selected_text_to_clipboard(target_pid: Option<i32>) -> Result<String, String> {
    let external_pid = target_pid
        .filter(|pid| *pid > 0 && *pid != std::process::id() as i32)
        .or_else(|| {
            ax::frontmost_pid().filter(|pid| *pid > 0 && *pid != std::process::id() as i32)
        });
    let copied = selected_text_copy_first(
        || {
            external_pid
                .ok_or_else(|| "无法确定选中文字所在的应用".to_owned())
                .and_then(copy_shortcut_to_pid)
        },
        || {
            target_pid
                .map(ax::selection_in_pid)
                .unwrap_or_else(ax::selection)
        },
    )?;
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    if !pasteboard.setString_forType(&text(&copied), &text("public.utf8-plain-text")) {
        return Err("选中文字已读取，但无法写入剪贴板".into());
    }
    Ok(copied)
}

fn should_preload_ollama(
    is_ollama: bool,
    model: &str,
    _input_is_empty: bool,
    preloading: Option<&str>,
    last_preload: Option<(&str, Instant)>,
    _now: Instant,
) -> bool {
    if !is_ollama || model.trim().is_empty() {
        return false;
    }
    if preloading == Some(model) {
        return false;
    }
    !last_preload.is_some_and(|(loaded, _)| loaded == model)
}

fn install_edit_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let main_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &text("主菜单"));
    let edit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &text("编辑"),
            None,
            &text(""),
        )
    };
    main_menu.addItem(&edit_item);
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &text("编辑"));
    edit_item.setSubmenu(Some(&edit_menu));
    let entries: [(&str, Option<Sel>, &str, u64); 6] = [
        ("撤销", Some(sel!(undo:)), "z", 0),
        ("重做", Some(sel!(redo:)), "Z", 0),
        ("剪切", Some(sel!(cut:)), "x", 0),
        ("拷贝", Some(sel!(copy:)), "c", 0),
        ("粘贴", Some(sel!(paste:)), "v", 0),
        ("全选", Some(sel!(selectAll:)), "a", 0),
    ];
    for (title, action, key, _unused) in entries {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &text(title),
                action,
                &text(key),
            )
        };
        edit_menu.addItem(&item);
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));
    main_menu
}

thread_local! { static STATE: RefCell<Option<App>> = const { RefCell::new(None) }; }
fn with_app(f: impl FnOnce(&mut App)) {
    STATE.with(|s| {
        if let Ok(mut state) = s.try_borrow_mut()
            && let Some(app) = state.as_mut()
            && !app.quitting
        {
            f(app);
        }
    });
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Actions;
    impl Actions {
        #[unsafe(method(tick:))] fn timer_tick(&self, _: &NSTimer) {
            STATE.with(|state| {
                if let Ok(mut state) = state.try_borrow_mut() && let Some(app) = state.as_mut() { app.tick(); }
            });
        }
        #[unsafe(method(recordShortcut:))] fn record_shortcut(&self, sender: &NSButton) { with_app(|a| a.begin_recording(sender.tag() as usize)); }
        #[unsafe(method(providerChanged:))] fn provider_changed(&self, sender: Option<&AnyObject>) {
            let picker = sender.and_then(|s| s.downcast_ref::<NSPopUpButton>());
            if let Some(picker) = picker { with_app(|a| a.provider_changed(picker)); }
        }
        #[unsafe(method(enableOnline:))] fn enable_online(&self, _: Option<&AnyObject>) { with_app(|a| a.enable_online()); }
        #[unsafe(method(settings:))] fn settings(&self, _: Option<&AnyObject>) { with_app(|a| a.settings()); }
        #[unsafe(method(togglePopup:))] fn toggle_popup(&self, _: Option<&AnyObject>) { with_app(|a| a.toggle_panel()); }
        #[unsafe(method(togglePinned:))] fn toggle_pinned(&self, _: Option<&AnyObject>) { with_app(|a| a.toggle_pinned()); }
        #[unsafe(method(translationToggle:))] fn translation_toggle(&self, sender: &NSButton) { with_app(|a| a.toggle_translation_slot(sender.tag() as usize)); }
        #[unsafe(method(toggleResult:))] fn toggle_result(&self, sender: &NSButton) { with_app(|a| a.toggle_result(sender.tag() as usize)); }
        #[unsafe(method(comparisonServiceChanged:))] fn comparison_service_changed(&self, sender: &NSPopUpButton) { with_app(|a| a.comparison_service_changed(sender)); }
        #[unsafe(method(copyResultCard:))] fn copy_result_card(&self, sender: &NSButton) { with_app(|a| a.copy_result_card(sender.tag() as usize)); }
        #[unsafe(method(openTranslation:))] fn open_translation(&self, _: Option<&AnyObject>) { with_app(|a| a.open_panel()); }
        #[unsafe(method(swapLanguages:))] fn swap_languages(&self, _: Option<&AnyObject>) { with_app(|a| a.swap_languages()); }
        #[unsafe(method(saveShortcuts:))] fn save_shortcuts(&self, _: Option<&AnyObject>) { with_app(|a| a.save_shortcuts()); }
        #[unsafe(method(saveSiliconFlow:))] fn save_siliconflow(&self, _: Option<&AnyObject>) { with_app(|a| a.save_siliconflow()); }
        #[unsafe(method(saveLibreTranslate:))] fn save_libretranslate(&self, _: Option<&AnyObject>) { with_app(|a| a.save_libretranslate()); }
        #[unsafe(method(saveCodexModel:))] fn save_codex_model(&self, _: Option<&AnyObject>) { with_app(|a| a.save_codex_model()); }
        #[unsafe(method(refreshOllamaModels:))] fn refresh_ollama_models(&self, _: Option<&AnyObject>) { with_app(|a| a.refresh_ollama_models()); }
        #[unsafe(method(pullOllamaModel:))] fn pull_ollama_model(&self, _: Option<&AnyObject>) { with_app(|a| a.pull_ollama_model()); }
        #[unsafe(method(cancelOllamaPull:))] fn cancel_ollama_pull(&self, _: Option<&AnyObject>) { with_app(|a| a.cancel_ollama_pull()); }
        #[unsafe(method(deleteOllamaModel:))] fn delete_ollama_model(&self, _: Option<&AnyObject>) { with_app(|a| a.delete_ollama_model()); }
        #[unsafe(method(browseOllamaModels:))] fn browse_ollama_models(&self, _: Option<&AnyObject>) { with_app(|a| a.browse_ollama_models()); }
        #[unsafe(method(defaultShortcuts:))] fn default_shortcuts(&self, _: Option<&AnyObject>) { with_app(|a| { for (field,key) in a.shortcut_fields.iter().zip(Config::default().shortcuts) { field.setStringValue(&text(&key.label())); } }); }
        #[unsafe(method(copyResult:))] fn copy(&self, _: Option<&AnyObject>) { with_app(|a| a.copy()); }
        #[unsafe(method(clear:))] fn clear(&self, _: Option<&AnyObject>) { with_app(|a| a.clear()); }
        #[unsafe(method(translate:))] fn translate(&self, _: Option<&AnyObject>) { with_app(|a| a.translate_manual()); }
        #[unsafe(method(selection:))] fn selection(&self, _: Option<&AnyObject>) { with_app(|a| a.selection()); }
        #[unsafe(method(screenshot:))] fn screenshot(&self, _: Option<&AnyObject>) { with_app(|a| a.screenshot()); }
        #[unsafe(method(languageChanged:))] fn language(&self, _: Option<&AnyObject>) { with_app(|a| { if a.is_reading { a.config.reading_target = a.language(); } else { a.config.target = a.language(); } let _ = a.config.save(); a.invalidate_translation(); }); }
        #[unsafe(method(setup:))]
        fn setup(&self, _: Option<&AnyObject>) {
            with_app(|a| {
                if a.config.provider == easytranslate::translation::Provider::LocalAi {
                    a.preload_local_model();
                } else {
                    a.settings();
                }
            });
        }
        #[unsafe(method(sourceChanged:))] fn source_changed(&self, _: Option<&AnyObject>) { with_app(|a| a.invalidate_translation()); }
        #[unsafe(method(permission:))] fn permission(&self, _: Option<&AnyObject>) { let _ = Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn(); }
        #[unsafe(method(quit:))] fn quit(&self, _: Option<&AnyObject>) { with_app(|a| { a.operation.begin(); a.quitting=true; a.draft.observe(None); a.cancel_generation.store(a.draft.generation, Ordering::Relaxed); a.status("正在取消后台任务并清理截图…"); }); }
    }
    unsafe impl NSObjectProtocol for Actions {}
    unsafe impl NSApplicationDelegate for Actions {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _: &NSApplication, _: bool) -> bool {
            with_app(|a| a.show_panel());
            false
        }
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn keep_running(&self, _: &NSApplication) -> bool { false }
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _: &NSApplication) -> NSApplicationTerminateReply {
            self.quit(sel!(quit:), None);
            NSApplicationTerminateReply::TerminateCancel
        }
    }
);

const LANGUAGES: [(&str, &str); 7] = [
    ("en", "English"),
    ("zh-Hans", "简体中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
];
// Start a request after a short typing pause so slow remote models can work
// while the user continues composing. Only one request is in flight at once.
const INPUT_DEBOUNCE: Duration = Duration::from_millis(90);
const MAIN_PANEL_WIDTH: f64 = 380.;
const TRANSLATION_CARD_HEADER_HEIGHT: f64 = 42.;
const TRANSLATION_CARD_GAP: f64 = 8.;
enum Event {
    TranslationSlot(u64, usize, Result<String, String>),
    Recognizing(u64),
    Ocr(u64, Result<String, String>),
    LocalPreload(Result<(), String>),
    OllamaPreload(String, Result<(), String>),
    OllamaModels(Result<Vec<String>, String>),
    OllamaPullProgress(String, Option<u64>, Option<u64>),
    OllamaPull(Result<String, String>),
    OllamaDelete(Result<String, String>),
}

struct TranslationCard {
    service_id: String,
    container: Retained<NSBox>,
    heading_button: Retained<NSButton>,
    selector: Retained<NSPopUpButton>,
    selector_service_ids: Vec<String>,
    output_scroll: Retained<NSScrollView>,
    output: Retained<NSTextView>,
    copy_button: Retained<NSButton>,
    collapsed: bool,
}

struct App {
    panel: Retained<NSPanel>,
    root: Retained<NSView>,
    background: Retained<NSVisualEffectView>,
    output: Retained<NSTextView>,
    input: Retained<NSTextView>,
    input_scroll: Retained<NSScrollView>,
    source_label: Retained<NSTextField>,
    target_label: Retained<NSTextField>,
    input_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    target: Retained<NSPopUpButton>,
    source: Retained<NSPopUpButton>,
    copy_button: Retained<NSButton>,
    translation_cards: Vec<TranslationCard>,
    translation_toggles: Vec<Retained<NSButton>>,
    pin_button: Retained<NSButton>,
    pinned: bool,
    settings_button: Retained<NSButton>,
    clear_button: Retained<NSButton>,
    retry_button: Retained<NSButton>,
    screenshot_button: Retained<NSButton>,
    _swap_button: Retained<NSButton>,
    _status_item: Retained<NSStatusItem>,
    _status_menu: Retained<NSMenu>,
    toggle_menu_item: Retained<NSMenuItem>,
    selection_menu_item: Retained<NSMenuItem>,
    screenshot_menu_item: Retained<NSMenuItem>,
    _actions: Retained<Actions>,
    config: Config,
    hotkeys: Option<hotkeys::Registry>,
    settings_window: Option<Retained<NSWindow>>,
    provider_picker: Option<Retained<NSPopUpButton>>,
    online_button: Option<Retained<NSButton>>,
    model_button: Option<Retained<NSButton>>,
    ollama_refresh_button: Option<Retained<NSButton>>,
    ollama_section_label: Option<Retained<NSTextField>>,
    ollama_model_label: Option<Retained<NSTextField>>,
    ollama_model_field: Option<Retained<NSTextField>>,
    ollama_pull_button: Option<Retained<NSButton>>,
    ollama_cancel_button: Option<Retained<NSButton>>,
    ollama_progress: Option<Retained<NSProgressIndicator>>,
    ollama_delete_button: Option<Retained<NSButton>>,
    ollama_browse_button: Option<Retained<NSButton>>,
    siliconflow_endpoint_field: Option<Retained<NSTextField>>,
    siliconflow_model_field: Option<Retained<NSTextField>>,
    siliconflow_api_key_field: Option<Retained<NSSecureTextField>>,
    siliconflow_labels: Vec<Retained<NSTextField>>,
    siliconflow_save_button: Option<Retained<NSButton>>,
    siliconflow_key_note: Option<Retained<NSTextField>>,
    libretranslate_labels: Vec<Retained<NSTextField>>,
    libretranslate_endpoint_field: Option<Retained<NSTextField>>,
    libretranslate_api_key_field: Option<Retained<NSSecureTextField>>,
    libretranslate_save_button: Option<Retained<NSButton>>,
    libretranslate_key_note: Option<Retained<NSTextField>>,
    codex_model_label: Option<Retained<NSTextField>>,
    codex_model_note: Option<Retained<NSTextField>>,
    codex_model_field: Option<Retained<NSPopUpButton>>,
    codex_model_save_button: Option<Retained<NSButton>>,
    shortcut_fields: Vec<Retained<NSTextField>>,
    record_buttons: Vec<Retained<NSButton>>,
    recording: Option<usize>,
    settings_status: Option<Retained<NSTextField>>,
    draft: Draft,
    is_reading: bool,
    operation: Operation,
    screenshot_path: Option<PathBuf>,
    quitting: bool,
    cache: Arc<std::sync::Mutex<Cache>>,
    local_translator: Arc<easytranslate::local_ai::LocalTranslator>,
    changed: Instant,
    attempted: Option<u64>,
    busy: bool,
    translation_generation: Option<u64>,
    translation_workers: usize,
    capture_busy: bool,
    ollama_pull_cancel: Option<Arc<AtomicBool>>,
    ollama_preloading: Option<String>,
    ollama_last_preload: Option<(String, Instant)>,
    ollama_loaded_models: Vec<String>,
    cancel_generation: Arc<AtomicU64>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
fn text(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
fn label(
    view: &NSView,
    title: &str,
    frame: NSRect,
    size: f64,
    bold: bool,
    muted: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&text(title), mtm);
    field.setFrame(frame);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    let color = if muted {
        NSColor::secondaryLabelColor()
    } else {
        NSColor::labelColor()
    };
    field.setTextColor(Some(&color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    view.addSubview(&field);
    field
}
fn input_field(
    view: &NSView,
    frame: NSRect,
    value: &str,
    placeholder: &str,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
    field.setStringValue(&text(value));
    field.setPlaceholderString(Some(&text(placeholder)));
    field.setFont(Some(&NSFont::systemFontOfSize(14.)));
    field.setEditable(true);
    field.setBezeled(true);
    field.setControlSize(NSControlSize::Small);
    field.setFocusRingType(NSFocusRingType::Exterior);
    view.addSubview(&field);
    field
}

fn style_popup(picker: &NSPopUpButton) {
    picker.setControlSize(NSControlSize::Small);
    picker.setFont(Some(&NSFont::systemFontOfSize(12.)));
}

fn set_disclosure_state(button: &NSButton, collapsed: bool) {
    let (symbol, label) = if collapsed {
        ("chevron.down", "展开译文")
    } else {
        ("chevron.up", "收起译文")
    };
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &text(symbol),
        Some(&text(label)),
    );
    button.setImage(image.as_deref());
    button.setToolTip(Some(&text(label)));
}

fn panel_box(view: &NSView, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSBox> {
    let box_view = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    box_view.setBoxType(NSBoxType::Custom);
    box_view.setTransparent(false);
    box_view.setFillColor(&NSColor::controlBackgroundColor());
    box_view.setBorderColor(&NSColor::separatorColor());
    box_view.setBorderWidth(1.);
    box_view.setCornerRadius(12.);
    box_view.setContentViewMargins(NSSize::new(0., 0.));
    view.addSubview(&box_view);
    box_view
}

fn button(
    view: &NSView,
    title: &str,
    frame: NSRect,
    action: Sel,
    target: &Actions,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let b: Retained<NSButton> = unsafe { msg_send![NSButton::alloc(mtm), initWithFrame: frame] };
    unsafe {
        let _: () = msg_send![&*b, setTitle: &*text(title)];
        let _: () = msg_send![&*b, setBezelStyle: NSBezelStyle::Push];
        let _: () = msg_send![&*b, setTarget: target];
        let _: () = msg_send![&*b, setAction: action];
    }
    b.setControlSize(NSControlSize::Small);
    b.setFont(Some(&NSFont::systemFontOfSize(12.)));
    view.addSubview(&b);
    b
}
fn menu_item(
    menu: &NSMenu,
    title: &str,
    action: Sel,
    target: &Actions,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &text(title),
            Some(action),
            &text(""),
        )
    };
    unsafe { item.setTarget(Some(target as &AnyObject)) };
    menu.addItem(&item);
    item
}
fn text_view(
    view: &NSView,
    frame: NSRect,
    editable: bool,
    mtm: MainThreadMarker,
) -> (Retained<NSScrollView>, Retained<NSTextView>) {
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
    scroll.setHasVerticalScroller(true);
    scroll.setBorderType(NSBorderType::LineBorder);
    scroll.setDrawsBackground(true);
    scroll.setScrollerStyle(NSScrollerStyle::Overlay);
    let background = NSColor::controlBackgroundColor();
    scroll.setBackgroundColor(&background);
    let content = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        rect(0., 0., frame.size.width - 18., frame.size.height),
    );
    content.setEditable(editable);
    content.setSelectable(true);
    content.setDrawsBackground(true);
    content.setBackgroundColor(&background);
    content.setRichText(false);
    content.setVerticallyResizable(true);
    if let Some(container) = unsafe { content.textContainer() } {
        container.setWidthTracksTextView(false);
        container.setHeightTracksTextView(false);
    }
    content.setFont(Some(&NSFont::systemFontOfSize(14.)));
    content.setTextColor(Some(&NSColor::labelColor()));
    content.setFocusRingType(NSFocusRingType::Exterior);
    unsafe {
        let _: () = msg_send![&*content, setTextContainerInset: NSSize::new(8.,8.)];
    }
    scroll.setDocumentView(Some(&content));
    view.addSubview(&scroll);
    (scroll, content)
}

fn checkbox(
    view: &NSView,
    title: &str,
    frame: NSRect,
    checked: bool,
    tag: usize,
    target: &Actions,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let checkbox = button(view, title, frame, sel!(translationToggle:), target, mtm);
    checkbox.setTag(tag as isize);
    checkbox.setButtonType(NSButtonType::Switch);
    checkbox.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    checkbox
}

fn comparison_toggle_title(service: &easytranslate::service::ServiceConfig) -> String {
    use easytranslate::service::ServiceKind;

    match service.kind {
        ServiceKind::LocalAi => "本地模型".into(),
        ServiceKind::Ollama => {
            let model = service.model.to_ascii_lowercase();
            if model.contains("12b") {
                "Ollama 12B".into()
            } else if model.contains("3b") {
                "Ollama 3B".into()
            } else {
                format!("Ollama {}", service.model)
            }
        }
        ServiceKind::Google => "Google".into(),
        ServiceKind::LibreTranslate => "Libre".into(),
        ServiceKind::OpenAiCli => "Codex".into(),
        ServiceKind::OpenAiCompatible => service.name.clone(),
    }
}

fn translation_card(
    root: &NSView,
    service_id: &str,
    index: usize,
    target: &Actions,
    mtm: MainThreadMarker,
) -> TranslationCard {
    let container = NSBox::initWithFrame(NSBox::alloc(mtm), rect(0., 0., 552., 100.));
    container.setBoxType(NSBoxType::Custom);
    container.setTransparent(false);
    container.setFillColor(&NSColor::controlBackgroundColor());
    container.setBorderColor(&NSColor::separatorColor());
    container.setBorderWidth(1.);
    container.setCornerRadius(10.);
    container.setContentViewMargins(NSSize::new(0., 0.));
    root.addSubview(&container);
    let selector = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(272., 68., 88., 28.),
        true,
    );
    selector.addItemWithTitle(&text("更换"));
    selector.setTag(index as isize);
    unsafe {
        let _: () = msg_send![&*selector, setTarget: target];
        let _: () = msg_send![&*selector, setAction: sel!(comparisonServiceChanged:)];
    }
    style_popup(&selector);
    container.addSubview(&selector);
    let heading_button = button(
        &container,
        "",
        rect(12., 66., 180., 28.),
        sel!(toggleResult:),
        target,
        mtm,
    );
    heading_button.setTag(index as isize);
    heading_button.setBordered(false);
    heading_button.setTransparent(false);
    heading_button.setShowsBorderOnlyWhileMouseInside(true);
    heading_button.setFont(Some(&NSFont::boldSystemFontOfSize(13.)));
    heading_button.setAlignment(NSTextAlignment::Left);
    heading_button.setImagePosition(NSCellImagePosition::ImageLeading);
    set_disclosure_state(&heading_button, true);
    let copy_button = button(
        &container,
        "复制",
        rect(320., 66., 70., 28.),
        sel!(copyResultCard:),
        target,
        mtm,
    );
    copy_button.setTag(index as isize);
    copy_button.setEnabled(false);
    let (output_scroll, output) = text_view(&container, rect(14., 8., 484., 54.), false, mtm);
    output_scroll.setHidden(true);
    TranslationCard {
        service_id: service_id.into(),
        container,
        heading_button,
        selector,
        selector_service_ids: Vec::new(),
        output_scroll,
        output,
        copy_button,
        collapsed: true,
    }
}

impl App {
    fn settings_message(&self, message: &str) {
        if let Some(label) = &self.settings_status {
            label.setStringValue(&text(message));
        }
    }
    fn finish_recording(&mut self, message: &str) {
        if self.recording.take().is_none() {
            return;
        }
        for button in &self.record_buttons {
            button.setTitle(&text("录制"));
        }
        let result = self
            .hotkeys
            .as_mut()
            .ok_or("监听未启动".to_string())
            .and_then(|registry| registry.configure(self.config.shortcuts));
        match result {
            Ok(()) => self.settings_message(message),
            Err(error) => {
                let message = format!("恢复快捷键失败：{error}；请重新保存快捷键");
                self.settings_message(&message);
                self.status(&message);
            }
        }
    }
    fn begin_recording(&mut self, index: usize) {
        if index >= self.shortcut_fields.len() {
            return;
        }
        if self.recording == Some(index) {
            self.finish_recording("已取消录制，原快捷键保持不变");
            return;
        }
        self.finish_recording("已切换录制项目");
        if let Some(window) = &self.settings_window {
            window.makeFirstResponder(None);
        }
        let Some(registry) = &mut self.hotkeys else {
            self.settings_message("快捷键监听未启动，请重启软件");
            return;
        };
        registry.suspend();
        self.recording = Some(index);
        self.record_buttons[index].setTitle(&text("取消"));
        self.settings_message("请按新的组合键（如 ⌥A）；Esc 取消，录制后点击保存");
    }
    fn recording_event(&mut self, event: &NSEvent) -> bool {
        let Some(index) = self.recording else {
            return false;
        };
        let Some(window) = &self.settings_window else {
            return false;
        };
        if !window.isKeyWindow() || !window.isVisible() {
            self.finish_recording("窗口失去焦点，已取消录制");
            return false;
        }
        if event.r#type() != NSEventType::KeyDown || event.windowNumber() != window.windowNumber() {
            return false;
        }
        if event.keyCode() == 53 {
            self.finish_recording("已取消录制，原快捷键保持不变");
        } else {
            match Shortcut::from_recorded_event(event.keyCode(), event.modifierFlags().bits()) {
                Ok(key) => {
                    self.shortcut_fields[index].setStringValue(&text(&key.label()));
                    self.finish_recording("已录制，请点击保存快捷键生效");
                }
                Err(error) => self.settings_message(&error),
            }
        }
        true
    }
    fn settings(&mut self) {
        self.finish_recording("已取消录制");
        let mtm = MainThreadMarker::new().unwrap();
        if self.settings_window.is_none() {
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect(120., 80., 560., 640.),
                    NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            window.setTitle(&text("EasyTranslate · 设置"));
            unsafe {
                window.setReleasedWhenClosed(false);
            }
            let root = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 560., 640.));
            window.setContentView(Some(&root));
            let settings_background = NSVisualEffectView::initWithFrame(
                NSVisualEffectView::alloc(mtm),
                rect(0., 0., 560., 640.),
            );
            settings_background.setMaterial(NSVisualEffectMaterial::WindowBackground);
            settings_background.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            settings_background.setState(NSVisualEffectState::Active);
            root.addSubview(&settings_background);
            label(
                &root,
                "EasyTranslate 设置",
                rect(28., 588., 500., 28.),
                21.,
                true,
                false,
                mtm,
            );
            label(
                &root,
                "翻译服务、模型和快捷键",
                rect(30., 566., 500., 18.),
                12.,
                false,
                true,
                mtm,
            );
            let _shortcuts_box = panel_box(&root, rect(20., 360., 520., 200.), mtm);
            let _service_box = panel_box(&root, rect(20., 258., 520., 92.), mtm);
            let _service_details_box = panel_box(&root, rect(20., 48., 520., 196.), mtm);
            label(
                &root,
                "快捷键",
                rect(36., 528., 200., 24.),
                16.,
                true,
                false,
                mtm,
            );
            label(
                &root,
                "输入、划词和截图翻译的全局快捷键",
                rect(36., 506., 400., 18.),
                12.,
                false,
                true,
                mtm,
            );
            for (i, title) in ["开关翻译框", "划词翻译", "截图翻译"].iter().enumerate()
            {
                let y = 468. - i as f64 * 34.;
                label(
                    &root,
                    title,
                    rect(36., y + 4., 120., 25.),
                    13.,
                    true,
                    false,
                    mtm,
                );
                let field = input_field(&root, rect(166., y, 260., 28.), "", "例如 Option+A", mtm);
                self.shortcut_fields.push(field);
                let record = button(
                    &root,
                    "录制",
                    rect(444., y, 80., 28.),
                    sel!(recordShortcut:),
                    &self._actions,
                    mtm,
                );
                record.setTag(i as isize);
                self.record_buttons.push(record);
            }
            let status = label(
                &root,
                "保存后立即生效；不能与其他功能或应用冲突",
                rect(36., 364., 488., 18.),
                12.,
                false,
                false,
                mtm,
            );
            self.settings_status = Some(status);
            label(
                &root,
                "服务设置",
                rect(36., 334., 200., 24.),
                16.,
                true,
                false,
                mtm,
            );
            label(
                &root,
                "选择服务后修改配置；翻译卡片可单独更换",
                rect(36., 274., 300., 16.),
                11.,
                false,
                true,
                mtm,
            );
            let picker = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                rect(36., 300., 488., 30.),
                false,
            );
            for service in &self.config.services {
                picker.addItemWithTitle(&text(&service.name));
            }
            unsafe {
                let _: () = msg_send![&*picker,setTarget:&*self._actions];
                let _: () = msg_send![&*picker,setAction:sel!(providerChanged:)];
            }
            style_popup(&picker);
            root.addSubview(&picker);
            self.provider_picker = Some(picker);
            self.online_button = Some(button(
                &root,
                "启用联网翻译",
                rect(354., 270., 170., 28.),
                sel!(enableOnline:),
                &self._actions,
                mtm,
            ));
            self.model_button = Some(button(
                &root,
                "准备本地模型",
                rect(354., 270., 170., 28.),
                sel!(setup:),
                &self._actions,
                mtm,
            ));
            let ollama_section_label = label(
                &root,
                "Ollama 模型管理",
                rect(36., 218., 150., 22.),
                14.,
                true,
                false,
                mtm,
            );
            self.ollama_section_label = Some(ollama_section_label);
            self.ollama_refresh_button = Some(button(
                &root,
                "刷新 Ollama 模型",
                rect(184., 216., 110., 26.),
                sel!(refreshOllamaModels:),
                &self._actions,
                mtm,
            ));
            let ollama_model_label = label(
                &root,
                "模型名 / 地址",
                rect(36., 180., 90., 22.),
                13.,
                true,
                false,
                mtm,
            );
            self.ollama_model_label = Some(ollama_model_label);
            let ollama_model_field = input_field(
                &root,
                rect(36., 150., 224., 28.),
                "",
                "qwen2.5:3b 或 hf.co/...",
                mtm,
            );
            self.ollama_model_field = Some(ollama_model_field);
            self.ollama_pull_button = Some(button(
                &root,
                "下载并添加",
                rect(268., 150., 104., 28.),
                sel!(pullOllamaModel:),
                &self._actions,
                mtm,
            ));
            let ollama_progress = NSProgressIndicator::initWithFrame(
                NSProgressIndicator::alloc(mtm),
                rect(36., 64., 300., 16.),
            );
            ollama_progress.setIndeterminate(false);
            ollama_progress.setMinValue(0.);
            ollama_progress.setMaxValue(100.);
            ollama_progress.setDoubleValue(0.);
            ollama_progress.setHidden(true);
            root.addSubview(&ollama_progress);
            self.ollama_progress = Some(ollama_progress);
            self.ollama_cancel_button = Some(button(
                &root,
                "取消下载",
                rect(378., 60., 96., 26.),
                sel!(cancelOllamaPull:),
                &self._actions,
                mtm,
            ));
            self.ollama_browse_button = Some(button(
                &root,
                "浏览 Ollama 模型库",
                rect(36., 110., 128., 26.),
                sel!(browseOllamaModels:),
                &self._actions,
                mtm,
            ));
            self.ollama_delete_button = Some(button(
                &root,
                "删除当前模型",
                rect(172., 110., 122., 26.),
                sel!(deleteOllamaModel:),
                &self._actions,
                mtm,
            ));
            self.siliconflow_labels.push(label(
                &root,
                "API 地址",
                rect(36., 218., 488., 18.),
                13.,
                true,
                false,
                mtm,
            ));
            let endpoint_field = input_field(
                &root,
                rect(36., 188., 488., 26.),
                &self.config.siliconflow_endpoint,
                "https://api.siliconflow.cn/v1/chat/completions",
                mtm,
            );
            self.siliconflow_endpoint_field = Some(endpoint_field);
            self.siliconflow_labels.push(label(
                &root,
                "模型名称",
                rect(36., 158., 488., 18.),
                13.,
                true,
                false,
                mtm,
            ));
            let model_field = input_field(
                &root,
                rect(36., 128., 488., 26.),
                &self.config.siliconflow_model,
                "Qwen/Qwen3-32B",
                mtm,
            );
            self.siliconflow_model_field = Some(model_field);
            self.siliconflow_labels.push(label(
                &root,
                "API Key",
                rect(36., 98., 488., 18.),
                13.,
                true,
                false,
                mtm,
            ));
            let api_key_field = NSSecureTextField::initWithFrame(
                NSSecureTextField::alloc(mtm),
                rect(36., 68., 488., 26.),
            );
            api_key_field.setStringValue(&text(&self.config.siliconflow_api_key));
            api_key_field.setPlaceholderString(Some(&text("从硅基流动控制台复制 API Key")));
            api_key_field.setFont(Some(&NSFont::systemFontOfSize(14.)));
            api_key_field.setEditable(true);
            api_key_field.setBezeled(true);
            root.addSubview(&api_key_field);
            self.siliconflow_api_key_field = Some(api_key_field);
            let siliconflow_save_button = button(
                &root,
                "保存硅基流动设置",
                rect(404., 54., 120., 26.),
                sel!(saveSiliconFlow:),
                &self._actions,
                mtm,
            );
            self.siliconflow_save_button = Some(siliconflow_save_button);
            let siliconflow_key_note = label(
                &root,
                "Key 保存在本机私有配置中；在线翻译授权开启后才会发送原文",
                rect(36., 54., 356., 18.),
                12.,
                false,
                true,
                mtm,
            );
            self.siliconflow_key_note = Some(siliconflow_key_note);
            self.libretranslate_labels.push(label(
                &root,
                "LibreTranslate 服务地址",
                rect(36., 218., 488., 18.),
                13.,
                true,
                false,
                mtm,
            ));
            let libretranslate_endpoint_field = input_field(
                &root,
                rect(36., 188., 488., 26.),
                "",
                "http://localhost:5000 或 https://实例地址",
                mtm,
            );
            self.libretranslate_endpoint_field = Some(libretranslate_endpoint_field);
            self.libretranslate_labels.push(label(
                &root,
                "API Key（可选）",
                rect(36., 158., 488., 18.),
                13.,
                true,
                false,
                mtm,
            ));
            let libretranslate_api_key_field = NSSecureTextField::initWithFrame(
                NSSecureTextField::alloc(mtm),
                rect(36., 128., 488., 26.),
            );
            libretranslate_api_key_field.setStringValue(&text(""));
            libretranslate_api_key_field.setPlaceholderString(Some(&text("实例要求时填写")));
            libretranslate_api_key_field.setFont(Some(&NSFont::systemFontOfSize(14.)));
            libretranslate_api_key_field.setEditable(true);
            libretranslate_api_key_field.setBezeled(true);
            root.addSubview(&libretranslate_api_key_field);
            self.libretranslate_api_key_field = Some(libretranslate_api_key_field);
            let libretranslate_save_button = button(
                &root,
                "保存 LibreTranslate",
                rect(398., 54., 126., 26.),
                sel!(saveLibreTranslate:),
                &self._actions,
                mtm,
            );
            self.libretranslate_save_button = Some(libretranslate_save_button);
            self.libretranslate_key_note = Some(label(
                &root,
                "开源接口；原文会发送到此服务地址",
                rect(36., 54., 356., 18.),
                11.,
                false,
                true,
                mtm,
            ));
            let codex_model_label = label(
                &root,
                "Codex 模型",
                rect(36., 218., 150., 20.),
                13.,
                true,
                false,
                mtm,
            );
            self.codex_model_label = Some(codex_model_label);
            let codex_model_field = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                rect(36., 172., 360., 28.),
                false,
            );
            codex_model_field.addItemWithTitle(&text("跟随本机 Codex 配置"));
            let current_codex_model = if self.config.llm_model.trim().is_empty() {
                easytranslate::llm::LlmSettings::from_codex()
                    .map(|settings| settings.model)
                    .unwrap_or_default()
            } else {
                self.config.llm_model.clone()
            };
            for model in easytranslate::llm::codex_model_choices(&current_codex_model) {
                codex_model_field.addItemWithTitle(&text(&model));
            }
            style_popup(&codex_model_field);
            root.addSubview(&codex_model_field);
            self.codex_model_field = Some(codex_model_field);
            let codex_model_save_button = button(
                &root,
                "保存模型",
                rect(414., 172., 110., 28.),
                sel!(saveCodexModel:),
                &self._actions,
                mtm,
            );
            self.codex_model_save_button = Some(codex_model_save_button);
            let codex_model_note = label(
                &root,
                &format!(
                    "留空跟随 Codex /model；当前：{}",
                    easytranslate::llm::LlmSettings::from_codex()
                        .map(|settings| settings.model)
                        .unwrap_or_else(|_| "本机 Codex 不可用".into())
                ),
                rect(36., 144., 488., 18.),
                12.,
                false,
                true,
                mtm,
            );
            self.codex_model_note = Some(codex_model_note);
            button(
                &root,
                "恢复默认",
                rect(36., 12., 104., 28.),
                sel!(defaultShortcuts:),
                &self._actions,
                mtm,
            );
            button(
                &root,
                "辅助功能权限",
                rect(148., 12., 126., 28.),
                sel!(permission:),
                &self._actions,
                mtm,
            );
            button(
                &root,
                "退出",
                rect(282., 12., 70., 28.),
                sel!(quit:),
                &self._actions,
                mtm,
            );
            button(
                &root,
                "保存快捷键",
                rect(370., 12., 154., 28.),
                sel!(saveShortcuts:),
                &self._actions,
                mtm,
            );
            self.settings_window = Some(window);
        }
        for (field, key) in self.shortcut_fields.iter().zip(self.config.shortcuts) {
            field.setStringValue(&text(&key.label()));
        }
        if let Some(field) = &self.siliconflow_endpoint_field {
            field.setStringValue(&text(&self.config.siliconflow_endpoint));
        }
        if let Some(field) = &self.siliconflow_model_field {
            field.setStringValue(&text(&self.config.siliconflow_model));
        }
        if let Some(field) = &self.siliconflow_api_key_field {
            field.setStringValue(&text(&self.config.siliconflow_api_key));
        }
        if let Some(service) = self
            .config
            .services
            .get(self.config.active_service)
            .filter(|service| service.kind == easytranslate::service::ServiceKind::LibreTranslate)
        {
            if let Some(field) = &self.libretranslate_endpoint_field {
                field.setStringValue(&text(&service.endpoint));
            }
            if let Some(field) = &self.libretranslate_api_key_field {
                field.setStringValue(&text(&service.api_key));
            }
        }
        if let Some(field) = &self.codex_model_field {
            let selected = self.config.llm_model.trim();
            let index = if selected.is_empty() {
                0
            } else {
                let title = text(selected);
                let found: isize = unsafe { msg_send![&**field, indexOfItemWithTitle: &*title] };
                if found >= 0 { found } else { 0 }
            };
            field.selectItemAtIndex(index);
        }
        self.refresh_provider_controls();
        if let Some(window) = &self.settings_window {
            window.makeKeyAndOrderFront(None);
        }
        NSApplication::sharedApplication(mtm).activate();
    }
    fn refresh_provider_controls(&mut self) {
        if let Some(picker) = self.provider_picker.as_ref() {
            unsafe {
                let _: () = msg_send![&**picker, removeAllItems];
                for service in &self.config.services {
                    picker.addItemWithTitle(&text(&service.name));
                }
                let _: () =
                    msg_send![&**picker,selectItemAtIndex:self.config.active_service as isize];
            }
        }
        let selected_kind = self
            .config
            .services
            .get(self.config.active_service)
            .map(|service| service.kind);
        let local_service = matches!(
            selected_kind,
            Some(
                easytranslate::service::ServiceKind::LocalAi
                    | easytranslate::service::ServiceKind::Ollama
            )
        );
        if let Some(button) = &self.online_button {
            button.setHidden(local_service);
            let title = if self.config.online_enabled {
                "关闭联网翻译授权"
            } else {
                match selected_kind {
                    Some(easytranslate::service::ServiceKind::OpenAiCompatible) => {
                        "启用联网翻译（原文会发给兼容服务）"
                    }
                    Some(easytranslate::service::ServiceKind::OpenAiCli) => {
                        "启用联网翻译（原文会发给 Codex 配置中的模型服务）"
                    }
                    _ => "启用联网翻译",
                }
            };
            button.setTitle(&text(title));
        }
        if let Some(button) = &self.model_button {
            button.setHidden(selected_kind != Some(easytranslate::service::ServiceKind::LocalAi));
            button.setTitle(&text("准备本地模型"));
        }
        let ollama_selected = selected_kind == Some(easytranslate::service::ServiceKind::Ollama);
        if let Some(button) = &self.ollama_refresh_button {
            button.setHidden(!ollama_selected);
        }
        if let Some(label) = &self.ollama_section_label {
            label.setHidden(!ollama_selected);
        }
        if let Some(label) = &self.ollama_model_label {
            label.setHidden(!ollama_selected);
        }
        if let Some(field) = &self.ollama_model_field {
            field.setHidden(!ollama_selected);
        }
        if let Some(button) = &self.ollama_pull_button {
            button.setHidden(!ollama_selected);
        }
        if let Some(button) = &self.ollama_cancel_button {
            button.setHidden(!ollama_selected || self.ollama_pull_cancel.is_none());
        }
        if let Some(progress) = &self.ollama_progress {
            progress.setHidden(!ollama_selected || self.ollama_pull_cancel.is_none());
        }
        if let Some(button) = &self.ollama_delete_button {
            button.setHidden(!ollama_selected);
        }
        if let Some(button) = &self.ollama_browse_button {
            button.setHidden(!ollama_selected);
        }
        let siliconflow_selected =
            selected_kind == Some(easytranslate::service::ServiceKind::OpenAiCompatible);
        for label in &self.siliconflow_labels {
            label.setHidden(!siliconflow_selected);
        }
        if let Some(field) = &self.siliconflow_endpoint_field {
            field.setHidden(!siliconflow_selected);
        }
        if let Some(field) = &self.siliconflow_model_field {
            field.setHidden(!siliconflow_selected);
        }
        if let Some(field) = &self.siliconflow_api_key_field {
            field.setHidden(!siliconflow_selected);
        }
        if let Some(button) = &self.siliconflow_save_button {
            button.setHidden(!siliconflow_selected);
        }
        if let Some(note) = &self.siliconflow_key_note {
            note.setHidden(!siliconflow_selected);
        }
        let libretranslate_selected =
            selected_kind == Some(easytranslate::service::ServiceKind::LibreTranslate);
        for label in &self.libretranslate_labels {
            label.setHidden(!libretranslate_selected);
        }
        if let Some(field) = &self.libretranslate_endpoint_field {
            field.setHidden(!libretranslate_selected);
        }
        if let Some(field) = &self.libretranslate_api_key_field {
            field.setHidden(!libretranslate_selected);
        }
        if let Some(button) = &self.libretranslate_save_button {
            button.setHidden(!libretranslate_selected);
        }
        if let Some(note) = &self.libretranslate_key_note {
            note.setHidden(!libretranslate_selected);
        }
        let codex_selected = selected_kind == Some(easytranslate::service::ServiceKind::OpenAiCli);
        if let Some(field) = &self.codex_model_field {
            field.setHidden(!codex_selected);
        }
        if let Some(button) = &self.codex_model_save_button {
            button.setHidden(!codex_selected);
        }
        if let Some(label) = &self.codex_model_label {
            label.setHidden(!codex_selected);
        }
        if let Some(note) = &self.codex_model_note {
            note.setHidden(!codex_selected);
        }
        self.refresh_comparison_selectors();
    }
    fn refresh_comparison_selectors(&mut self) {
        for (index, card) in self.translation_cards.iter_mut().enumerate() {
            if let Some(service) = self
                .config
                .services
                .iter()
                .find(|service| service.id == card.service_id)
                && let Some(toggle) = self.translation_toggles.get(index)
            {
                toggle.setTitle(&text(&comparison_toggle_title(service)));
                toggle.setToolTip(Some(&text(&service.name)));
            }
            unsafe {
                let _: () = msg_send![&*card.selector, removeAllItems];
            }
            card.selector.addItemWithTitle(&text("更换"));
            let mut choices =
                easytranslate::service::comparison_service_choices(&self.config.services);
            if choices.is_empty() {
                choices.push(card.service_id.clone());
            }
            for service_id in &choices {
                if let Some(service) = self
                    .config
                    .services
                    .iter()
                    .find(|service| &service.id == service_id)
                {
                    card.selector.addItemWithTitle(&text(&service.name));
                }
            }
            card.selector_service_ids = choices;
            card.selector.selectItemAtIndex(0);
        }
    }
    fn comparison_service_changed(&mut self, selector: &NSPopUpButton) {
        let index = selector.tag() as usize;
        let selected_index = selector.indexOfSelectedItem();
        if index >= self.translation_cards.len() {
            return;
        }
        if selected_index <= 0 {
            self.refresh_comparison_selectors();
            return;
        }
        let Some(service_id) = self.translation_cards[index]
            .selector_service_ids
            .get(selected_index as usize - 1)
            .cloned()
        else {
            self.refresh_comparison_selectors();
            return;
        };
        let mut config = self.config.clone();
        let Some(updated) = easytranslate::service::assign_comparison_service(
            &config.services,
            &config.comparison_services,
            index,
            &service_id,
        ) else {
            self.refresh_comparison_selectors();
            return;
        };
        config.comparison_services = updated;
        if let Err(error) = config.save() {
            self.status(&format!("保存翻译卡片设置失败：{error}"));
            self.refresh_comparison_selectors();
            return;
        }
        self.config = config;
        for (card, service_id) in self
            .translation_cards
            .iter_mut()
            .zip(self.config.comparison_services.iter())
        {
            card.service_id = service_id.clone();
        }
        self.invalidate_translation();
        self.refresh_comparison_selectors();
        if self.selected_comparison_uses(easytranslate::service::ServiceKind::LocalAi) {
            self.preload_local_model();
        }
        self.preload_selected_ollama();
        self.relayout();
    }
    fn provider_changed(&mut self, picker: &NSPopUpButton) {
        let index: isize = unsafe { msg_send![picker, indexOfSelectedItem] };
        if index < 0 || index as usize >= self.config.services.len() {
            return;
        }
        let mut config = self.config.clone();
        config.active_service = index as usize;
        config.provider = config.services[index as usize].provider();
        self.save_provider_config(config);
        if let Some(service) = self
            .config
            .services
            .get(self.config.active_service)
            .filter(|service| service.kind == easytranslate::service::ServiceKind::LibreTranslate)
        {
            if let Some(field) = &self.libretranslate_endpoint_field {
                field.setStringValue(&text(&service.endpoint));
            }
            if let Some(field) = &self.libretranslate_api_key_field {
                field.setStringValue(&text(&service.api_key));
            }
        }
        if self.panel.isVisible() {
            self.preload_selected_ollama();
        }
    }
    fn enable_online(&mut self) {
        let mut config = self.config.clone();
        config.online_enabled = !config.online_enabled;
        self.save_provider_config(config);
    }
    fn save_siliconflow(&mut self) {
        let (Some(endpoint), Some(model), Some(api_key)) = (
            &self.siliconflow_endpoint_field,
            &self.siliconflow_model_field,
            &self.siliconflow_api_key_field,
        ) else {
            return;
        };
        let mut config = self.config.clone();
        config.siliconflow_endpoint = endpoint.stringValue().to_string().trim().into();
        config.siliconflow_model = model.stringValue().to_string().trim().into();
        config.siliconflow_api_key = api_key.stringValue().to_string().trim().into();
        match config.save() {
            Ok(()) => {
                self.config = config;
                self.invalidate_translation();
                self.status("硅基流动 API 设置已保存");
                self.settings_message("硅基流动 API 地址、模型和 Key 已保存到本机配置");
            }
            Err(error) => self.status(&format!("硅基流动设置保存失败：{error}")),
        }
    }
    fn save_libretranslate(&mut self) {
        let (Some(endpoint), Some(api_key)) = (
            &self.libretranslate_endpoint_field,
            &self.libretranslate_api_key_field,
        ) else {
            return;
        };
        let Some(service) = self.config.services.get(self.config.active_service) else {
            return;
        };
        if service.kind != easytranslate::service::ServiceKind::LibreTranslate {
            return;
        }
        let mut config = self.config.clone();
        let service = &mut config.services[config.active_service];
        service.endpoint = endpoint
            .stringValue()
            .to_string()
            .trim()
            .trim_end_matches('/')
            .into();
        service.api_key = api_key.stringValue().to_string().trim().into();
        match config.save() {
            Ok(()) => {
                self.config = config;
                self.invalidate_translation();
                self.status("LibreTranslate 设置已保存");
                self.settings_message("服务地址和可选 API Key 已保存到本机配置");
            }
            Err(error) => self.status(&format!("LibreTranslate 设置保存失败：{error}")),
        }
    }
    fn save_codex_model(&mut self) {
        let Some(field) = &self.codex_model_field else {
            return;
        };
        let mut config = self.config.clone();
        let index: isize = unsafe { msg_send![&**field, indexOfSelectedItem] };
        config.llm_model = if index <= 0 {
            String::new()
        } else {
            field
                .titleOfSelectedItem()
                .map(|title| title.to_string().trim().to_string())
                .unwrap_or_default()
        };
        if let Some(service) = config.services.get_mut(config.active_service)
            && service.kind == easytranslate::service::ServiceKind::OpenAiCli
        {
            service.model = config.llm_model.clone();
        }
        match config.save() {
            Ok(()) => {
                self.config = config;
                self.invalidate_translation();
                self.status("本机 Codex 模型设置已保存");
                self.settings_message("留空时跟随本机 Codex 配置；模型名已保存到本机设置");
            }
            Err(error) => self.status(&format!("Codex 模型设置保存失败：{error}")),
        }
    }
    fn save_provider_config(&mut self, config: Config) {
        match config.save() {
            Ok(()) => {
                self.config = config;
                self.invalidate_translation();
                self.status("服务设置已保存");
                if let Some(label) = &self.settings_status {
                    label.setStringValue(&text("服务设置已保存；模型设置可在下方修改。"));
                }
            }
            Err(error) => self.status(&format!("设置保存失败：{error}")),
        }
        self.refresh_provider_controls();
    }
    fn refresh_ollama_models(&mut self) {
        let tx = self.tx.clone();
        self.status("正在读取本机 Ollama 模型");
        thread::spawn(move || {
            let _ = tx.send(Event::OllamaModels(easytranslate::llm::ollama_models()));
        });
    }

    fn browse_ollama_models(&mut self) {
        match Command::new("/usr/bin/open")
            .arg("https://ollama.com/library")
            .spawn()
        {
            Ok(_) => self.status("已打开 Ollama 模型库"),
            Err(error) => self.status(&format!("无法打开 Ollama 模型库：{error}")),
        }
    }

    fn pull_ollama_model(&mut self) {
        if self.ollama_pull_cancel.is_some() {
            return;
        }
        let Some(field) = &self.ollama_model_field else {
            return;
        };
        let model = field.stringValue().to_string();
        if model.trim().is_empty() {
            self.status("请输入 Ollama 模型名称或模型地址");
            return;
        }
        if let Some(button) = &self.ollama_pull_button {
            button.setEnabled(false);
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        self.ollama_pull_cancel = Some(cancellation.clone());
        if let Some(button) = &self.ollama_cancel_button {
            button.setHidden(false);
            button.setEnabled(true);
        }
        if let Some(progress) = &self.ollama_progress {
            progress.setHidden(false);
            progress.setDoubleValue(0.);
        }
        self.refresh_provider_controls();
        self.status("正在连接 Ollama，准备下载…");
        self.settings_message("正在连接 Ollama，准备下载模型…");
        let tx = self.tx.clone();
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = easytranslate::llm::ollama_pull_with_progress(
                &model,
                || !cancellation.load(Ordering::Relaxed),
                move |progress| {
                    let _ = progress_tx.send(Event::OllamaPullProgress(
                        progress.status,
                        progress.completed,
                        progress.total,
                    ));
                },
            );
            let _ = tx.send(Event::OllamaPull(result));
        });
    }

    fn cancel_ollama_pull(&mut self) {
        let Some(cancellation) = &self.ollama_pull_cancel else {
            return;
        };
        cancellation.store(true, Ordering::Relaxed);
        if let Some(button) = &self.ollama_cancel_button {
            button.setEnabled(false);
        }
        self.status("正在取消 Ollama 下载…");
        self.settings_message("正在取消下载并关闭连接…");
    }

    fn delete_ollama_model(&mut self) {
        let Some(service) = self.config.services.get(self.config.active_service) else {
            return;
        };
        if service.kind != easytranslate::service::ServiceKind::Ollama {
            self.status("请先在翻译服务菜单中选择一个 Ollama 模型");
            return;
        }
        let model = service.model.clone();
        let mtm = MainThreadMarker::new().unwrap();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&text("删除 Ollama 模型？"));
        alert.setInformativeText(&text(&format!(
            "将从本机 Ollama 删除「{model}」，下载文件也会被移除。"
        )));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.addButtonWithTitle(&text("删除"));
        alert.addButtonWithTitle(&text("取消"));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        if let Some(button) = &self.ollama_delete_button {
            button.setEnabled(false);
        }
        self.status("正在删除 Ollama 模型…");
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Event::OllamaDelete(easytranslate::llm::ollama_delete(
                &model,
            )));
        });
    }
    fn preload_local_model(&self) {
        let local_translator = self.local_translator.clone();
        let tx = self.tx.clone();
        let target = self.config.target.clone();
        let source = if target == "zh-Hans" {
            "en".to_string()
        } else {
            "zh-Hans".to_string()
        };
        self.status("正在后台准备本地翻译模型");
        thread::spawn(move || {
            let result = local_translator.preload(&source, &target);
            let _ = tx.send(Event::LocalPreload(result));
        });
    }
    fn selected_comparison_uses(&self, kind: easytranslate::service::ServiceKind) -> bool {
        self.config
            .comparison_services
            .iter()
            .enumerate()
            .any(|(index, id)| {
                self.translation_toggles
                    .get(index)
                    .is_some_and(|toggle| toggle.state() == NSControlStateValueOn)
                    && self
                        .config
                        .services
                        .iter()
                        .any(|service| service.id == *id && service.kind == kind)
            })
    }
    fn selected_ollama_model(&self) -> Option<&str> {
        self.config
            .comparison_services
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                self.translation_toggles
                    .get(*index)
                    .is_some_and(|toggle| toggle.state() == NSControlStateValueOn)
            })
            .find_map(|(_, id)| {
                self.config
                    .services
                    .iter()
                    .find(|service| {
                        service.id == *id
                            && service.kind == easytranslate::service::ServiceKind::Ollama
                    })
                    .map(|service| service.model.as_str())
            })
    }
    fn preload_selected_ollama(&mut self) {
        let Some(model) = self.selected_ollama_model().map(str::to_owned) else {
            return;
        };
        let last_preload = self
            .ollama_last_preload
            .as_ref()
            .map(|(loaded, at)| (loaded.as_str(), *at));
        if !should_preload_ollama(
            true,
            &model,
            self.input.string().to_string().trim().is_empty(),
            self.ollama_preloading.as_deref(),
            last_preload,
            Instant::now(),
        ) {
            return;
        }
        self.ollama_preloading = Some(model.clone());
        self.status(&format!("正在后台加载 Ollama 模型 {model}…"));
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = easytranslate::llm::ollama_load(&model);
            let _ = tx.send(Event::OllamaPreload(model, result));
        });
    }
    fn save_shortcuts(&mut self) {
        self.finish_recording("录制已结束");
        let result = (|| -> Result<(), String> {
            let parsed = self
                .shortcut_fields
                .iter()
                .map(|f| Shortcut::parse(&f.stringValue().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let keys: [Shortcut; 3] = parsed.try_into().map_err(|_| "快捷键设置不完整")?;
            let registry = self
                .hotkeys
                .as_mut()
                .ok_or("全局快捷键监听未启动，请重启软件")?;
            let old = self.config.shortcuts;
            registry.configure(keys)?;
            let mut config = self.config.clone();
            config.shortcuts = keys;
            if let Err(error) = config.save() {
                registry
                    .configure(old)
                    .map_err(|rollback| format!("保存失败：{error}；恢复失败：{rollback}"))?;
                return Err(format!("保存失败，已保留原快捷键：{error}"));
            }
            self.config = config;
            self.update_shortcut_menu_labels();
            Ok(())
        })();
        let message = match result {
            Ok(()) => "已保存并立即生效；按新的快捷键打开翻译窗口".into(),
            Err(e) => e,
        };
        if let Some(label) = &self.settings_status {
            label.setStringValue(&text(&message));
        }
        self.status(&message);
    }
    fn update_shortcut_menu_labels(&self) {
        self.toggle_menu_item.setTitle(&text(&shortcut_menu_title(
            "开关翻译框",
            self.config.shortcuts[0],
        )));
        self.selection_menu_item
            .setTitle(&text(&shortcut_menu_title(
                "划词翻译",
                self.config.shortcuts[1],
            )));
        self.screenshot_menu_item
            .setTitle(&text(&shortcut_menu_title(
                "截图翻译",
                self.config.shortcuts[2],
            )));
    }
    fn new(mtm: MainThreadMarker) -> Self {
        let actions: Retained<Actions> = unsafe { msg_send![Actions::class(), new] };
        let _main_menu = install_edit_menu(mtm);
        let (config, config_error) = match Config::load() {
            Ok(config) => (config, None),
            Err(error) => (
                Config::default(),
                Some(format!("配置读取失败，原文件已保留：{error}")),
            ),
        };
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect(100., 150., MAIN_PANEL_WIDTH, 460.),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setTitle(&text("EasyTranslate"));
        panel.setLevel(3);
        panel.setHidesOnDeactivate(true);
        unsafe {
            panel.setReleasedWhenClosed(false);
        }
        let root = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., MAIN_PANEL_WIDTH, 460.));
        panel.setContentView(Some(&root));
        let background = NSVisualEffectView::initWithFrame(
            NSVisualEffectView::alloc(mtm),
            rect(0., 0., MAIN_PANEL_WIDTH, 460.),
        );
        background.setMaterial(NSVisualEffectMaterial::WindowBackground);
        background.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        background.setState(NSVisualEffectState::Active);
        root.addSubview(&background);
        let source_label = label(
            &root,
            "原文",
            rect(18., 404., 100., 16.),
            11.,
            false,
            true,
            mtm,
        );
        let target_label = label(
            &root,
            "翻译成",
            rect(154., 404., 100., 16.),
            11.,
            false,
            true,
            mtm,
        );
        let source = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(18., 370., 96., 28.),
            false,
        );
        source.addItemWithTitle(&text("自动识别 / Auto"));
        for (_, name) in LANGUAGES {
            source.addItemWithTitle(&text(name));
        }
        unsafe {
            let _: () = msg_send![&*source, selectItemAtIndex: 2isize];
            let _: () = msg_send![&*source, setTarget: &*actions];
            let _: () = msg_send![&*source, setAction: sel!(sourceChanged:)];
        }
        style_popup(&source);
        root.addSubview(&source);
        let target = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(154., 370., 96., 28.),
            false,
        );
        for (_, name) in LANGUAGES {
            target.addItemWithTitle(&text(name));
        }
        unsafe {
            let _: () = msg_send![&*target, selectItemAtIndex: LANGUAGES.iter().position(|(code,_)| *code == config.target).unwrap_or(0) as isize];
            let _: () = msg_send![&*target, setTarget: &*actions];
            let _: () = msg_send![&*target, setAction: sel!(languageChanged:)];
        }
        style_popup(&target);
        root.addSubview(&target);
        let swap_button = button(
            &root,
            "⇄",
            rect(122., 370., 28., 28.),
            sel!(swapLanguages:),
            &actions,
            mtm,
        );
        let input_label = label(
            &root,
            "输入原文",
            rect(18., 334., 260., 22.),
            13.,
            true,
            false,
            mtm,
        );
        let (input_scroll, input) = text_view(&root, rect(18., 210., 484., 104.), true, mtm);
        let translation_toggles = config
            .comparison_services
            .iter()
            .enumerate()
            .map(|(index, _service_id)| {
                checkbox(
                    &root,
                    &format!("译文 {}", index + 1),
                    rect(18. + index as f64 * 122., 72., 116., 28.),
                    true,
                    index,
                    &actions,
                    mtm,
                )
            })
            .collect::<Vec<_>>();
        let translation_cards = config
            .comparison_services
            .iter()
            .enumerate()
            .map(|(index, service_id)| translation_card(&root, service_id, index, &actions, mtm))
            .collect::<Vec<_>>();
        let output = translation_cards[0].output.clone();
        let status_label = label(
            &root,
            config_error.as_deref().unwrap_or(
                "输入原文后自动翻译；可选择本地模型、Ollama、Google、LibreTranslate 或 Codex",
            ),
            rect(18., 32., 484., 18.),
            11.,
            false,
            true,
            mtm,
        );
        let copy_button = translation_cards[0].copy_button.clone();
        let clear_button = button(
            &root,
            "清空",
            rect(300., 332., 52., 24.),
            sel!(clear:),
            &actions,
            mtm,
        );
        let retry_button = button(
            &root,
            "重试",
            rect(358., 332., 52., 24.),
            sel!(translate:),
            &actions,
            mtm,
        );
        let screenshot_button = button(
            &root,
            "截图",
            rect(416., 332., 58., 24.),
            sel!(screenshot:),
            &actions,
            mtm,
        );
        let settings_button = button(
            &root,
            "设置",
            rect(428., 370., 74., 28.),
            sel!(settings:),
            &actions,
            mtm,
        );
        let pin_button = button(
            &root,
            "置顶",
            rect(408., 62., 94., 28.),
            sel!(togglePinned:),
            &actions,
            mtm,
        );

        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(28.);
        status_item.setAutosaveName(Some(&text("EasyTranslate.MainMenu")));
        status_item.setVisible(true);
        if let Some(b) = status_item.button(mtm) {
            let icon_path = std::env::current_exe().ok().and_then(|p| {
                p.parent()?
                    .parent()
                    .map(|p| p.join("Resources/MenuBarTemplate.png"))
            });
            if let Some(image) = icon_path.and_then(|p| {
                NSImage::initWithContentsOfFile(NSImage::alloc(), &text(&p.to_string_lossy()))
            }) {
                image.setSize(NSSize::new(18., 18.));
                image.setTemplate(true);
                b.setImage(Some(&image));
                b.setTitle(&text(""));
                b.setAccessibilityLabel(Some(&text("译")));
            } else {
                b.setTitle(&text("译"));
            }
            b.setToolTip(Some(&text("EasyTranslate · 打开或关闭翻译")));
        }
        let status_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &text("EasyTranslate"));
        let toggle_menu_item = menu_item(
            &status_menu,
            &shortcut_menu_title("开关翻译框", config.shortcuts[0]),
            sel!(togglePopup:),
            &actions,
            mtm,
        );
        let selection_menu_item = menu_item(
            &status_menu,
            &shortcut_menu_title("划词翻译", config.shortcuts[1]),
            sel!(selection:),
            &actions,
            mtm,
        );
        let screenshot_menu_item = menu_item(
            &status_menu,
            &shortcut_menu_title("截图翻译", config.shortcuts[2]),
            sel!(screenshot:),
            &actions,
            mtm,
        );
        status_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let _ = menu_item(&status_menu, "设置", sel!(settings:), &actions, mtm);
        let _ = menu_item(
            &status_menu,
            "退出 EasyTranslate",
            sel!(quit:),
            &actions,
            mtm,
        );
        status_item.setMenu(Some(&status_menu));
        let (tx, rx) = mpsc::channel();
        let local_translator = Arc::new(easytranslate::local_ai::LocalTranslator::new(
            easytranslate::local_ai::LocalTranslator::default_cache_root(),
        ));
        let mut app = Self {
            panel,
            root,
            background,
            output,
            input,
            input_scroll,
            source_label,
            target_label,
            input_label,
            status_label,
            target,
            source,
            copy_button,
            translation_cards,
            translation_toggles,
            pin_button,
            pinned: false,
            settings_button,
            clear_button,
            retry_button,
            screenshot_button,
            _swap_button: swap_button,
            _status_item: status_item,
            _status_menu: status_menu,
            toggle_menu_item,
            selection_menu_item,
            screenshot_menu_item,
            _actions: actions,
            config,
            hotkeys: None,
            settings_window: None,
            provider_picker: None,
            online_button: None,
            model_button: None,
            ollama_refresh_button: None,
            ollama_section_label: None,
            ollama_model_label: None,
            ollama_model_field: None,
            ollama_pull_button: None,
            ollama_cancel_button: None,
            ollama_progress: None,
            ollama_delete_button: None,
            ollama_browse_button: None,
            siliconflow_endpoint_field: None,
            siliconflow_model_field: None,
            siliconflow_api_key_field: None,
            siliconflow_labels: Vec::new(),
            siliconflow_save_button: None,
            siliconflow_key_note: None,
            libretranslate_labels: Vec::new(),
            libretranslate_endpoint_field: None,
            libretranslate_api_key_field: None,
            libretranslate_save_button: None,
            libretranslate_key_note: None,
            codex_model_field: None,
            codex_model_save_button: None,
            codex_model_label: None,
            codex_model_note: None,
            shortcut_fields: Vec::new(),
            record_buttons: Vec::new(),
            recording: None,
            settings_status: None,
            draft: Draft::default(),
            is_reading: false,
            operation: Operation::default(),
            screenshot_path: None,
            quitting: false,
            cache: Arc::new(std::sync::Mutex::new(Cache::new(32))),
            local_translator,
            changed: Instant::now(),
            attempted: None,
            busy: false,
            translation_generation: None,
            translation_workers: 0,
            capture_busy: false,
            ollama_pull_cancel: None,
            ollama_preloading: None,
            ollama_last_preload: None,
            ollama_loaded_models: Vec::new(),
            cancel_generation: Arc::new(AtomicU64::new(0)),
            tx,
            rx,
        };
        app.refresh_comparison_selectors();
        app.relayout();
        app
    }
    fn text_document_height(view: &NSTextView, width: f64) -> f64 {
        let Some(container) = (unsafe { view.textContainer() }) else {
            return 64.;
        };
        container.setWidthTracksTextView(false);
        container.setHeightTracksTextView(false);
        container.setContainerSize(NSSize::new(width, f64::MAX));
        let Some(layout) = (unsafe { view.layoutManager() }) else {
            return 64.;
        };
        layout.ensureLayoutForTextContainer(&container);
        let used = layout.usedRectForTextContainer(&container);
        (used.size.height + 28.).max(64.)
    }

    fn relayout(&self) {
        let width = self.panel.frame().size.width.max(MAIN_PANEL_WIDTH);
        let input_document_height = Self::text_document_height(&self.input, width - 72.);
        let input_height = input_document_height.min(156.);
        let mut enabled_cards = vec![false; self.translation_cards.len()];
        let mut natural_card_heights = vec![0.; self.translation_cards.len()];
        let mut document_heights = vec![64.; self.translation_cards.len()];
        let mut cards_height = 0.;
        let mut expanded_count = 0;
        for (index, card) in self.translation_cards.iter().enumerate() {
            let enabled = self
                .translation_toggles
                .get(index)
                .is_some_and(|button| button.state() == NSControlStateValueOn);
            enabled_cards[index] = enabled;
            card.container.setHidden(!enabled);
            if !enabled {
                continue;
            }
            let card_height = if card.collapsed {
                TRANSLATION_CARD_HEADER_HEIGHT
            } else {
                expanded_count += 1;
                let document_height = Self::text_document_height(&card.output, width - 100.);
                document_heights[index] = document_height;
                document_height.min(220.) + TRANSLATION_CARD_HEADER_HEIGHT
            };
            natural_card_heights[index] = card_height;
            cards_height += card_height + TRANSLATION_CARD_GAP;
        }
        // Toolbar, input header, service row, and footer occupy a fixed rhythm;
        // only the input and result cards grow with content.
        let base_height = 220. + input_height;
        let desired_height = (base_height + cards_height).clamp(380., 820.);
        let enabled_count = enabled_cards.iter().filter(|enabled| **enabled).count();
        let available_content = (desired_height
            - base_height
            - enabled_count as f64 * (TRANSLATION_CARD_HEADER_HEIGHT + TRANSLATION_CARD_GAP))
            .max(1.);
        let content_limit = if expanded_count == 0 {
            0.
        } else {
            (available_content / expanded_count as f64).clamp(1., 220.)
        };
        let card_heights = natural_card_heights
            .iter()
            .enumerate()
            .map(|(index, natural_height)| {
                if !enabled_cards[index] {
                    0.
                } else if self.translation_cards[index].collapsed {
                    TRANSLATION_CARD_HEADER_HEIGHT
                } else {
                    TRANSLATION_CARD_HEADER_HEIGHT
                        + (natural_height - TRANSLATION_CARD_HEADER_HEIGHT).min(content_limit)
                }
            })
            .collect::<Vec<_>>();
        let frame = self.panel.frame();
        if (frame.size.height - desired_height).abs() > 1. {
            let next = NSRect::new(
                NSPoint::new(
                    frame.origin.x,
                    frame.origin.y - (desired_height - frame.size.height),
                ),
                NSSize::new(width, desired_height),
            );
            self.panel.setFrame_display_animate(next, true, false);
            self.root.setFrameSize(NSSize::new(width, desired_height));
        }
        let height = desired_height;
        self.background.setFrame(rect(0., 0., width, height));
        let top = height - 52.;
        self.source_label.setFrame(rect(18., top - 18., 100., 16.));
        self.target_label.setFrame(rect(154., top - 18., 100., 16.));
        self.source.setFrame(rect(18., top - 52., 96., 28.));
        self._swap_button.setFrame(rect(122., top - 52., 28., 28.));
        self.target.setFrame(rect(154., top - 52., 96., 28.));
        self.settings_button
            .setFrame(rect(width - 92., top - 52., 74., 28.));
        let input_label_y = top - 82.;
        self.status_label.setFrame(rect(18., 16., width - 36., 18.));
        self.input_label
            .setFrame(rect(18., input_label_y, width - 220., 22.));
        self.clear_button
            .setFrame(rect(width - 192., input_label_y - 1., 52., 24.));
        self.retry_button
            .setFrame(rect(width - 134., input_label_y - 1., 52., 24.));
        self.screenshot_button
            .setFrame(rect(width - 76., input_label_y - 1., 58., 24.));
        let input_y = input_label_y - 10. - input_height;
        self.input_scroll
            .setFrame(rect(18., input_y, width - 36., input_height));
        self.input.setFrameSize(NSSize::new(
            width - 72.,
            input_document_height.max(input_height),
        ));
        let choice_y = input_y - 34.;
        self.pin_button
            .setFrame(rect(width - 94., choice_y, 76., 28.));
        for (index, toggle) in self.translation_toggles.iter().enumerate() {
            toggle.setFrame(rect(18. + index as f64 * 88., choice_y, 86., 28.));
        }
        let mut cursor = choice_y - TRANSLATION_CARD_GAP;
        for (index, card) in self.translation_cards.iter().enumerate() {
            if !enabled_cards[index] {
                continue;
            }
            let card_height = card_heights[index];
            cursor -= card_height;
            card.container
                .setFrame(rect(18., cursor, width - 36., card_height));
            let card_width = width - 36.;
            let action_y = card_height - 32.;
            let copy_x = card_width - 84.;
            let selector_x = copy_x - 96.;
            card.heading_button
                .setFrame(rect(12., action_y, (selector_x - 18.).max(132.), 28.));
            card.selector.setFrame(rect(selector_x, action_y, 88., 28.));
            let service_name = self
                .config
                .services
                .iter()
                .find(|service| service.id == card.service_id)
                .map(comparison_toggle_title)
                .unwrap_or_else(|| "翻译服务".into());
            card.heading_button.setTitle(&text(&service_name));
            card.copy_button.setFrame(rect(copy_x, action_y, 70., 28.));
            set_disclosure_state(&card.heading_button, card.collapsed);
            card.output_scroll.setHidden(card.collapsed);
            if !card.collapsed {
                let output_height = card_height - TRANSLATION_CARD_HEADER_HEIGHT;
                card.output_scroll
                    .setFrame(rect(14., 4., width - 64., output_height));
                card.output.setFrameSize(NSSize::new(
                    width - 100.,
                    document_heights[index].max(output_height),
                ));
            }
            cursor -= TRANSLATION_CARD_GAP;
        }
    }
    fn language(&self) -> String {
        LANGUAGES
            .get(self.target.indexOfSelectedItem() as usize)
            .unwrap_or(&LANGUAGES[0])
            .0
            .into()
    }
    fn source_language(&self) -> String {
        let index = self.source.indexOfSelectedItem();
        if index > 0 {
            LANGUAGES[(index - 1) as usize].0.into()
        } else {
            "auto".into()
        }
    }
    fn set_mode(&mut self, reading: bool) {
        if self.is_reading == reading {
            return;
        }
        self.is_reading = reading;
        let target = if reading {
            &self.config.reading_target
        } else {
            &self.config.target
        };
        let target_index = LANGUAGES
            .iter()
            .position(|(code, _)| *code == target)
            .unwrap_or(if reading { 1 } else { 0 });
        unsafe {
            let _: () = msg_send![&*self.target, selectItemAtIndex: target_index as isize];
            let _: () =
                msg_send![&*self.source, selectItemAtIndex: if reading { 0isize } else { 2isize }];
        }
        self.invalidate_translation();
    }
    fn status(&self, message: &str) {
        self.status_label.setStringValue(&text(message));
    }
    fn show_panel(&mut self) {
        self.preload_selected_ollama();
        self.position_popup();
        self.panel.makeKeyAndOrderFront(None);
        let application = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
        application.activate();
        self.panel.orderFrontRegardless();
        self.panel.makeFirstResponder(Some(&self.input));
    }
    fn toggle_panel(&mut self) {
        let app_frontmost = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .is_some_and(|app| app.processIdentifier() == std::process::id() as i32);
        if toggle_action(
            self.panel.isVisible(),
            app_frontmost,
            self.panel.isKeyWindow(),
        ) {
            self.show_panel();
        } else {
            self.panel.orderOut(None);
        }
    }
    fn toggle_pinned(&mut self) {
        self.pinned = !self.pinned;
        self.panel.setHidesOnDeactivate(!self.pinned);
        self.panel.setFloatingPanel(self.pinned);
        self.panel
            .setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
        self.pin_button.setTitle(&text(if self.pinned {
            "取消置顶"
        } else {
            "置顶"
        }));
        if self.pinned {
            self.panel.makeKeyAndOrderFront(None);
        }
    }
    fn toggle_translation_slot(&mut self, index: usize) {
        if index >= self.translation_toggles.len() {
            return;
        }
        let enabled_count = self
            .translation_toggles
            .iter()
            .filter(|button| button.state() == NSControlStateValueOn)
            .count();
        if enabled_count == 1 && self.translation_toggles[index].state() == NSControlStateValueOn {
            self.status("至少保留一个翻译服务");
            return;
        }
        self.invalidate_translation();
        if self.selected_comparison_uses(easytranslate::service::ServiceKind::LocalAi) {
            self.preload_local_model();
        }
        self.preload_selected_ollama();
        self.relayout();
    }
    fn toggle_result(&mut self, index: usize) {
        if let Some(card) = self.translation_cards.get_mut(index) {
            card.collapsed = !card.collapsed;
            self.relayout();
        }
    }
    fn copy_result_card(&mut self, index: usize) {
        let Some(card) = self.translation_cards.get(index) else {
            return;
        };
        let value = card.output.string().to_string();
        let service_name = self
            .config
            .services
            .iter()
            .find(|service| service.id == card.service_id)
            .map(|service| service.name.as_str())
            .unwrap_or("服务");
        if value.trim().is_empty() {
            self.status("译文为空，无法复制");
            return;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if pasteboard.setString_forType(&text(&value), &text("public.utf8-plain-text")) {
            self.status(&format!("{service_name}译文已复制"));
        } else {
            self.status("复制失败，请重试");
        }
    }
    fn open_panel(&mut self) {
        if !self.panel.isVisible() {
            self.set_mode(false);
        }
        self.show_panel();
    }
    fn position_popup(&self) {
        let mtm = MainThreadMarker::new().unwrap();
        let Some(main) = NSScreen::mainScreen(mtm) else {
            return;
        };
        let visible = main.visibleFrame();
        let frame = self.panel.frame();
        let (x, y) = popup_origin(
            Rect::new(
                visible.origin.x,
                visible.origin.y,
                visible.size.width,
                visible.size.height,
            ),
            frame.size.width,
            frame.size.height,
        );
        self.panel.setFrameOrigin(NSPoint::new(x, y));
    }
    fn set_preview_text(&self, value: &str) {
        self.output.setString(&text(value));
    }
    fn invalidate_translation(&mut self) {
        self.operation.begin();
        self.draft.observe(None);
        self.busy = false;
        self.translation_generation = None;
        self.cancel_generation
            .store(self.draft.generation, Ordering::Relaxed);
        self.copy_button.setEnabled(false);
        self.set_preview_text("");
        for card in &mut self.translation_cards {
            card.output.setString(&text(""));
            card.output.setTextColor(Some(&NSColor::labelColor()));
            card.copy_button.setEnabled(false);
            card.collapsed = true;
        }
        self.attempted = None;
        self.status("正在等待原文输入…");
        self.relayout();
    }
    fn clear(&mut self) {
        self.input.setString(&text(""));
        self.invalidate_translation();
        self.panel.makeFirstResponder(Some(&self.input));
        self.relayout();
    }
    fn swap_languages(&mut self) {
        let Some(current) = self.draft.current.as_ref() else {
            self.status("请先完成一次翻译，再交换语言");
            return;
        };
        if self.draft.translation.is_none() {
            self.status("请先等待译文完成，再交换语言");
            return;
        }
        let original = current.text.clone();
        let translated = self.output.string().to_string();
        if translated.trim().is_empty() {
            self.status("译文为空，无法交换语言");
            return;
        }
        let old_target = self.language();
        let source_index = self.source.indexOfSelectedItem();
        let old_source = if source_index > 0 {
            LANGUAGES
                .get((source_index - 1) as usize)
                .map(|(code, _)| (*code).to_owned())
        } else {
            match bridge::run(Request {
                action: "detect".into(),
                text: Some(original),
                source: None,
                target: None,
                path: None,
            }) {
                Ok(result) => result.text,
                Err(error) => {
                    self.status(&format!("无法识别原文语言，未交换：{error}"));
                    return;
                }
            }
        };
        let Some(old_source) = old_source.filter(|code| *code != old_target) else {
            self.status("原文与译文语言相同，无法交换");
            return;
        };
        let Some(source_index) = LANGUAGES.iter().position(|(code, _)| *code == old_target) else {
            self.status("不支持当前译文语言交换");
            return;
        };
        let Some(target_index) = LANGUAGES.iter().position(|(code, _)| *code == old_source) else {
            self.status("不支持当前原文语言交换");
            return;
        };
        unsafe {
            let _: () = msg_send![&*self.source, selectItemAtIndex: (source_index + 1) as isize];
            let _: () = msg_send![&*self.target, selectItemAtIndex: target_index as isize];
        }
        self.is_reading = false;
        self.config.target = old_source;
        if let Err(error) = self.config.save() {
            self.status(&format!("语言已交换，但设置保存失败：{error}"));
        }
        self.input.setString(&text(&translated));
        self.invalidate_translation();
        self.observe_input();
        self.panel.makeFirstResponder(Some(&self.input));
    }
    fn copy_selection_event(&self, event: &NSEvent) -> bool {
        let modifiers = event.modifierFlags()
            & (NSEventModifierFlags::Command
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Shift);
        if !self.panel.isKeyWindow()
            || event.windowNumber() != self.panel.windowNumber()
            || modifiers != NSEventModifierFlags::Command
            || (event.keyCode() != 8
                && !event
                    .charactersIgnoringModifiers()
                    .is_some_and(|characters| characters.to_string().eq_ignore_ascii_case("c")))
            || NSTextInputClient::selectedRange(&*self.output).length == 0
            || !self
                .panel
                .firstResponder()
                .is_some_and(|responder| std::ptr::eq(&*responder, &*self.output as &NSResponder))
        {
            return false;
        }
        unsafe { self.output.copy(None) };
        true
    }
    fn copy(&mut self) {
        let snapshot = Snapshot {
            target: 0,
            text: self.input.string().to_string(),
            language: self.language(),
        };
        let composing: bool = unsafe { msg_send![&*self.input, hasMarkedText] };
        if composing || self.draft.replacement(&snapshot).is_none() {
            self.status("没有可复制的有效译文");
            return;
        }
        let result = self.output.string().to_string();
        if result.trim().is_empty() {
            self.status("译文为空，无法复制");
            return;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if !pasteboard.setString_forType(&text(&result), &text("public.utf8-plain-text")) {
            self.status("复制失败，请重试");
            return;
        }
        self.status("译文已复制");
        self.panel.orderOut(None);
    }
    fn translate_manual(&mut self) {
        self.invalidate_translation();
        self.changed = Instant::now() - Duration::from_secs(1);
        self.show_panel();
    }
    fn selection(&mut self) {
        self.selection_for_target(None);
    }
    fn selection_for_target(&mut self, target_pid: Option<i32>) -> bool {
        let value = selected_text_to_clipboard(target_pid);
        match value {
            Ok(value) => {
                self.use_selection(value);
                true
            }
            Err(e) => {
                self.show_panel();
                self.status(&e);
                false
            }
        }
    }
    fn use_selection(&mut self, value: String) {
        self.paste_source_text(value);
    }
    fn paste_source_text(&mut self, value: String) {
        if value.trim().is_empty() {
            self.status("没有可翻译的文字");
            return;
        }
        self.set_mode(true);
        unsafe {
            let _: () = msg_send![&*self.source, selectItemAtIndex: 0isize];
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if !pasteboard.setString_forType(&text(&value), &text("public.utf8-plain-text")) {
            self.status("文字已识别，但无法复制到原文输入框");
            return;
        }
        self.show_panel();
        // The panel may not have completed its first-responder transition yet.
        // Setting the value directly avoids the race where paste() targets the
        // previous application even though the selection is already on the clipboard.
        self.input.setString(&text(&value));
        self.panel.makeFirstResponder(Some(&self.input));
        self.invalidate_translation();
        self.changed = Instant::now() - Duration::from_secs(1);
        self.relayout();
    }
    fn screenshot(&mut self) {
        if self.capture_busy {
            return;
        }
        self.set_mode(true);
        self.input.setString(&text(""));
        self.capture_busy = true;
        self.invalidate_translation();
        let generation = self.operation.begin();
        let path = std::env::temp_dir().join(format!(
            "easytranslate-{}-{}.png",
            std::process::id(),
            generation
        ));
        self.screenshot_path = Some(path.clone());
        self.panel.orderOut(None);
        let tx = self.tx.clone();
        let cancellation = self.operation.cancellation();
        thread::spawn(move || {
            let result = (|| {
                if cancellation.0.load(Ordering::Relaxed) != cancellation.1 {
                    return Err("截图已取消".into());
                }
                let mut child = Command::new("/usr/sbin/screencapture")
                    .args(["-i", "-x", "-t", "png"])
                    .arg(&path)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let status = easytranslate::capture::wait(&mut child, || {
                    cancellation.0.load(Ordering::Relaxed) != cancellation.1
                })?;
                if !status.success() || !path.exists() {
                    return Err("截图已取消，或没有录屏权限".into());
                }
                let _ = tx.send(Event::Recognizing(generation));
                bridge::run_cancellable(
                    Request {
                        action: "ocr".into(),
                        text: None,
                        source: None,
                        target: None,
                        path: Some(path.to_string_lossy().into_owned()),
                    },
                    Some(cancellation.clone()),
                )?
                .text
                .ok_or("未识别到文字".into())
            })();
            let _ = std::fs::remove_file(path);
            let _ = tx.send(Event::Ocr(generation, result));
        });
    }
    fn shutdown(&mut self) {
        self.cancel_generation.fetch_add(1, Ordering::Relaxed);
        if let Some(path) = self.screenshot_path.take() {
            let _ = std::fs::remove_file(path);
        }
        let selected_model = self
            .config
            .services
            .get(self.config.active_service)
            .filter(|service| service.kind == easytranslate::service::ServiceKind::Ollama)
            .map(|service| service.model.clone());
        if let Some(model) = selected_model
            && !self
                .ollama_loaded_models
                .iter()
                .any(|loaded| loaded == &model)
        {
            self.ollama_loaded_models.push(model);
        }
        for model in self.ollama_loaded_models.drain(..) {
            let _ = easytranslate::llm::ollama_unload(&model);
        }
    }
    fn observe_input(&mut self) {
        let composing: bool = unsafe { msg_send![&*self.input, hasMarkedText] };
        if composing {
            return;
        }
        let snapshot = Snapshot {
            target: 0,
            text: self.input.string().to_string(),
            language: self.language(),
        };
        if self.draft.observe(Some(snapshot)) {
            self.cancel_generation
                .store(self.draft.generation, Ordering::Relaxed);
            self.busy = false;
            self.translation_generation = None;
            self.changed = Instant::now();
            self.set_preview_text("");
            self.copy_button.setEnabled(false);
            if self.draft.current.is_some() {
                self.status("正在等待输入完成…");
            } else {
                self.status("输入原文后自动翻译");
            }
            self.relayout();
        }
    }
    fn options_for_translation_service(
        &self,
        service_id: &str,
    ) -> Result<easytranslate::translation::Options, String> {
        let service = self
            .config
            .services
            .iter()
            .find(|service| service.id == service_id)
            .ok_or_else(|| "翻译卡片对应的服务已不存在，请重新选择模型".to_string())?;
        let provider = service.provider();
        let model = service
            .model_for_translation(&self.config.llm_model)
            .to_string();
        let endpoint = if service.kind == easytranslate::service::ServiceKind::OpenAiCompatible
            && !service.endpoint.trim().is_empty()
        {
            service.endpoint.clone()
        } else {
            self.config.siliconflow_endpoint.clone()
        };
        let siliconflow_model = if service.kind
            == easytranslate::service::ServiceKind::OpenAiCompatible
            && !service.model.trim().is_empty()
        {
            service.model.clone()
        } else {
            self.config.siliconflow_model.clone()
        };
        let api_key = if service.kind == easytranslate::service::ServiceKind::OpenAiCompatible
            && !service.api_key.trim().is_empty()
        {
            service.api_key.clone()
        } else {
            self.config.siliconflow_api_key.clone()
        };
        let (libretranslate_endpoint, libretranslate_api_key) =
            if service.kind == easytranslate::service::ServiceKind::LibreTranslate {
                (service.endpoint.clone(), service.api_key.clone())
            } else {
                (String::new(), String::new())
            };
        Ok(easytranslate::translation::Options {
            provider,
            online_enabled: self.config.online_enabled,
            siliconflow_endpoint: endpoint,
            siliconflow_model,
            siliconflow_api_key: api_key,
            libretranslate_endpoint,
            libretranslate_api_key,
            codex_model: model,
            local_translator: Some(self.local_translator.clone()),
            tone: self.config.tone,
            glossary: self.config.glossary.clone(),
        })
    }
    fn tick(&mut self) {
        if self.recording.is_some()
            && self
                .settings_window
                .as_ref()
                .is_none_or(|w| !w.isKeyWindow() || !w.isVisible())
        {
            self.finish_recording("窗口失去焦点，已取消录制");
        }
        self.cancel_generation
            .store(self.draft.generation, Ordering::Relaxed);
        let (action, target_pid) = if self.quitting {
            (0, None)
        } else {
            self.hotkeys
                .as_ref()
                .map(|h| h.take_with_target())
                .unwrap_or((0, None))
        };
        match action {
            1 => self.toggle_panel(),
            2 => {
                self.selection_for_target(target_pid);
            }
            3 => self.screenshot(),
            _ => {}
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::TranslationSlot(generation, index, result) => {
                    // A cancelled request may finish after a newer input has
                    // already started translating. Its event must not consume
                    // a worker slot belonging to the newer generation. During
                    // shutdown `translation_generation` still points at the
                    // active request, so those workers are drained normally
                    // even though the draft generation has advanced.
                    if self.translation_generation != Some(generation) {
                        continue;
                    }
                    self.translation_workers = self.translation_workers.saturating_sub(1);
                    if let Some(card) = self.translation_cards.get_mut(index) {
                        match result {
                            Ok(value) => {
                                card.output.setString(&text(&value));
                                card.output.setTextColor(Some(&NSColor::labelColor()));
                                card.copy_button.setEnabled(!value.trim().is_empty());
                                card.collapsed = false;
                                if index == 0 {
                                    self.draft.accept(generation, value.clone());
                                    self.set_preview_text(&value);
                                    self.copy_button.setEnabled(!value.trim().is_empty());
                                }
                            }
                            Err(error) => {
                                card.output.setString(&text(&format!("翻译失败：{error}")));
                                card.output.setTextColor(Some(&NSColor::systemRedColor()));
                                card.copy_button.setEnabled(false);
                                card.collapsed = false;
                            }
                        }
                    }
                    if self.translation_workers == 0 {
                        self.busy = false;
                        self.translation_generation = None;
                        self.status("翻译完成");
                    }
                    self.relayout();
                }
                Event::LocalPreload(result) => {
                    if self.selected_comparison_uses(easytranslate::service::ServiceKind::LocalAi) {
                        match result {
                            Ok(()) => self.status("本地 Marian 模型已预热"),
                            Err(error) => {
                                self.status(&format!("本地 Marian 模型预载失败：{error}"))
                            }
                        }
                    }
                }
                Event::OllamaPreload(model, result) => {
                    if result.is_ok() && !self.ollama_loaded_models.contains(&model) {
                        self.ollama_loaded_models.push(model.clone());
                    }
                    let was_current_preload = self.ollama_preloading.as_deref() == Some(&model);
                    if was_current_preload {
                        self.ollama_preloading = None;
                    }
                    if self.selected_ollama_model() == Some(model.as_str()) {
                        if self.panel.isVisible()
                            && self.input.string().to_string().trim().is_empty()
                        {
                            match result {
                                Ok(()) => {
                                    self.ollama_last_preload =
                                        Some((model.clone(), Instant::now()));
                                    self.status(&format!("Ollama 模型 {model} 已在后台就绪"));
                                }
                                Err(error) => {
                                    self.status(&format!("Ollama 模型预加载失败：{error}"))
                                }
                            }
                        } else if result.is_ok() {
                            self.ollama_last_preload = Some((model.clone(), Instant::now()));
                        }
                    } else if was_current_preload && self.panel.isVisible() {
                        self.preload_selected_ollama();
                    }
                }
                Event::OllamaModels(Ok(models)) => {
                    let selected_id = self
                        .config
                        .services
                        .get(self.config.active_service)
                        .map(|service| service.id.clone());
                    let mut config = self.config.clone();
                    easytranslate::service::sync_ollama_models(&mut config.services, &models);
                    easytranslate::service::normalize_comparison_services(
                        &config.services,
                        &mut config.comparison_services,
                    );
                    config.active_service = selected_id
                        .and_then(|id| config.services.iter().position(|service| service.id == id))
                        .unwrap_or(0);
                    config.provider = config.services[config.active_service].provider();
                    match config.save() {
                        Ok(()) => {
                            let count = models.len();
                            self.config = config;
                            for (card, service_id) in self
                                .translation_cards
                                .iter_mut()
                                .zip(self.config.comparison_services.iter())
                            {
                                card.service_id = service_id.clone();
                            }
                            self.invalidate_translation();
                            self.refresh_provider_controls();
                            if self
                                .settings_window
                                .as_ref()
                                .is_some_and(|window| window.isVisible())
                            {
                                self.status(&format!("已同步 {count} 个 Ollama 本地模型"));
                                self.settings_message(&format!(
                                    "已同步 {count} 个 Ollama 本地模型，翻译页面选择菜单已更新。"
                                ));
                            }
                        }
                        Err(error) => self.status(&format!("保存本地模型列表失败：{error}")),
                    }
                }
                Event::OllamaModels(Err(error)) => {
                    let ollama_selected = self
                        .config
                        .services
                        .get(self.config.active_service)
                        .is_some_and(|service| {
                            service.kind == easytranslate::service::ServiceKind::Ollama
                        });
                    let settings_visible = self
                        .settings_window
                        .as_ref()
                        .is_some_and(|window| window.isVisible());
                    if ollama_selected || settings_visible {
                        self.status(&format!("读取 Ollama 模型失败：{error}"));
                        self.settings_message("请确认 Ollama 正在运行，然后刷新本地模型列表。");
                    }
                }
                Event::OllamaPullProgress(status, completed, total) => {
                    if self.ollama_pull_cancel.is_some() {
                        if let (Some(completed), Some(total)) =
                            (completed, total.filter(|n| *n > 0))
                        {
                            let percent =
                                ((completed as f64 / total as f64) * 100.).clamp(0., 100.);
                            if let Some(progress) = &self.ollama_progress {
                                progress.setDoubleValue(percent);
                            }
                            let message = format!(
                                "Ollama 下载：{status} · {percent:.0}%（{} / {}）",
                                format_bytes(completed),
                                format_bytes(total)
                            );
                            self.status(&message);
                            self.settings_message(&message);
                        } else {
                            let message = format!("Ollama 下载：{status}");
                            self.status(&message);
                            self.settings_message(&message);
                        }
                    }
                }
                Event::OllamaPull(Ok(model)) => {
                    self.ollama_pull_cancel = None;
                    if let Some(button) = &self.ollama_pull_button {
                        button.setEnabled(true);
                    }
                    if let Some(button) = &self.ollama_cancel_button {
                        button.setHidden(true);
                        button.setEnabled(true);
                    }
                    if let Some(progress) = &self.ollama_progress {
                        progress.setHidden(true);
                        progress.setDoubleValue(100.);
                    }
                    self.refresh_provider_controls();
                    if let Some(field) = &self.ollama_model_field {
                        field.setStringValue(&text(""));
                    }
                    self.status(&format!("Ollama 模型 {model} 下载完成，正在刷新列表…"));
                    self.settings_message(&format!(
                        "Ollama 模型 {model} 下载完成，正在更新翻译菜单…"
                    ));
                    self.refresh_ollama_models();
                }
                Event::OllamaPull(Err(error)) => {
                    self.ollama_pull_cancel = None;
                    if let Some(button) = &self.ollama_pull_button {
                        button.setEnabled(true);
                    }
                    if let Some(button) = &self.ollama_cancel_button {
                        button.setHidden(true);
                        button.setEnabled(true);
                    }
                    if let Some(progress) = &self.ollama_progress {
                        progress.setHidden(true);
                    }
                    self.refresh_provider_controls();
                    self.status(&error);
                    self.settings_message(if error.contains("已取消") {
                        "Ollama 模型下载已取消。"
                    } else {
                        "Ollama 模型下载失败，请检查模型名称、地址、本机服务和网络连接。"
                    });
                }
                Event::OllamaDelete(Ok(model)) => {
                    if let Some(button) = &self.ollama_delete_button {
                        button.setEnabled(true);
                    }
                    self.status(&format!("Ollama 模型 {model} 已删除，正在刷新列表…"));
                    self.refresh_ollama_models();
                }
                Event::OllamaDelete(Err(error)) => {
                    if let Some(button) = &self.ollama_delete_button {
                        button.setEnabled(true);
                    }
                    self.status(&error);
                    self.settings_message("Ollama 模型删除失败，请确认本机 Ollama 正在运行。");
                }
                Event::Recognizing(generation) => {
                    if self.operation.accepts(generation) && !self.quitting {
                        self.status("正在识别截图；首次可能较慢");
                    }
                }
                Event::Ocr(generation, result) => {
                    self.capture_busy = false;
                    self.screenshot_path = None;
                    if !self.operation.accepts(generation) {
                        continue;
                    }
                    match result {
                        Ok(value) => self.paste_source_text(value),
                        Err(e) => {
                            self.show_panel();
                            self.status(&e);
                        }
                    }
                }
            }
        }
        if self.quitting {
            if !self.capture_busy && self.translation_workers == 0 {
                self.shutdown();
                std::process::exit(0);
            }
            return;
        }
        self.observe_input();
        if !self.busy
            && self.changed.elapsed() > INPUT_DEBOUNCE
            && self.attempted != Some(self.draft.generation)
            && let Some(snapshot) = self.draft.current.clone()
        {
            self.busy = true;
            let generation = self.draft.generation;
            self.translation_generation = Some(generation);
            self.attempted = Some(generation);
            let selected_count = self
                .translation_toggles
                .iter()
                .filter(|button| button.state() == NSControlStateValueOn)
                .count();
            self.translation_workers = selected_count;
            self.status(&format!("正在并行翻译 {selected_count} 个服务…"));
            let source = self.source_language();
            let cancellation = self.cancel_generation.clone();
            cancellation.store(generation, Ordering::Relaxed);
            let cache = self.cache.clone();
            for (index, card) in self.translation_cards.iter().enumerate() {
                let enabled = self
                    .translation_toggles
                    .get(index)
                    .is_some_and(|button| button.state() == NSControlStateValueOn);
                if !enabled {
                    continue;
                }
                let tx = self.tx.clone();
                let result = self.options_for_translation_service(&card.service_id);
                let snapshot = snapshot.clone();
                let source = source.clone();
                let cancellation = cancellation.clone();
                let cache = cache.clone();
                thread::spawn(move || {
                    let result = result.and_then(|options| {
                        easytranslate::translation::run(
                            options,
                            &snapshot.text,
                            &source,
                            &snapshot.language,
                            (cancellation, generation),
                            cache,
                            |_, _| {},
                        )
                        .map(|translation| translation.text)
                    });
                    let _ = tx.send(Event::TranslationSlot(generation, index, result));
                });
            }
        }
    }
}
pub fn smoke() {
    let mtm = MainThreadMarker::new().expect("UI self-check must run on the main thread");
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let mut app = App::new(mtm);
    app.refresh_provider_controls();
    app.config.target = "en".into();
    app.config.reading_target = "zh-Hans".into();
    unsafe {
        let _: () = msg_send![&*app.target, selectItemAtIndex: 0isize];
        let _: () = msg_send![&*app.source, selectItemAtIndex: 2isize];
    }
    assert!(!app.panel.isVisible());
    let can_become_key: bool = unsafe { msg_send![&*app.panel, canBecomeKeyWindow] };
    assert!(can_become_key);
    let status_button = app._status_item.button(mtm).expect("menu bar button");
    let status_menu = app
        ._status_item
        .menu(mtm)
        .expect("menu bar icon must show its options menu");
    for (title, selector) in [
        (
            shortcut_menu_title("开关翻译框", app.config.shortcuts[0]),
            sel!(togglePopup:),
        ),
        (
            shortcut_menu_title("划词翻译", app.config.shortcuts[1]),
            sel!(selection:),
        ),
        (
            shortcut_menu_title("截图翻译", app.config.shortcuts[2]),
            sel!(screenshot:),
        ),
        ("设置".into(), sel!(settings:)),
        ("退出 EasyTranslate".into(), sel!(quit:)),
    ] {
        let item = status_menu
            .itemWithTitle(&text(&title))
            .unwrap_or_else(|| panic!("menu must include {title}"));
        assert_eq!(item.action(), Some(selector), "menu action for {title}");
        let target = item.target().expect("menu action target");
        assert!(
            std::ptr::eq(&*target, &*app._actions as &AnyObject),
            "menu action for {title} must target Actions"
        );
        let responds: bool = unsafe { msg_send![&*app._actions, respondsToSelector: selector] };
        assert!(responds, "Actions must implement {selector:?}");
    }
    assert!(
        app._status_item.isVisible(),
        "menu bar item must be visible"
    );
    assert!(status_button.image().is_some() || !status_button.title().to_string().is_empty());
    assert!(app.input.isEditable());
    assert!(app.output.isSelectable());
    assert_eq!(app.translation_cards.len(), 3);
    assert_eq!(app.translation_toggles.len(), 3);
    assert_eq!(app.panel.frame().size.width, MAIN_PANEL_WIDTH);
    assert!(unsafe { msg_send![&*app._actions, respondsToSelector: sel!(togglePinned:)] });
    for (index, service_id) in app.config.comparison_services.iter().enumerate() {
        let service = app
            .config
            .services
            .iter()
            .find(|service| &service.id == service_id)
            .expect("comparison toggle service");
        assert_eq!(
            app.translation_toggles[index]
                .toolTip()
                .unwrap()
                .to_string(),
            service.name,
            "comparison toggle must identify its selected translation service"
        );
        assert_eq!(
            app.translation_toggles[index].title().to_string(),
            comparison_toggle_title(service),
            "comparison toggle must use a compact service label"
        );
    }
    for toggle in &app.translation_toggles {
        toggle.setState(NSControlStateValueOn);
    }
    app.relayout();
    for (index, card) in app.translation_cards.iter().enumerate() {
        let service = app
            .config
            .services
            .iter()
            .find(|service| service.id == card.service_id)
            .expect("comparison card service");
        assert!(
            card.heading_button
                .title()
                .to_string()
                .contains(&comparison_toggle_title(service)),
            "card heading must show its selected service label"
        );
        assert!(
            !card.heading_button.isHidden(),
            "card heading must be visible"
        );
        assert!(card.heading_button.frame().size.width >= 128.);
        let title_is_button: bool =
            unsafe { msg_send![&*card.heading_button, isKindOfClass: NSButton::class()] };
        assert!(
            title_is_button,
            "service heading and disclosure action must be one control"
        );
        assert!(
            card.heading_button.image().is_some(),
            "translation heading must include a system disclosure symbol"
        );
        assert_eq!(
            card.heading_button.imagePosition(),
            NSCellImagePosition::ImageLeading
        );
        assert_eq!(
            card.selector.titleOfSelectedItem().unwrap().to_string(),
            "更换"
        );
        let title_frame = card.heading_button.frame();
        let selector_frame = card.selector.frame();
        let copy_frame = card.copy_button.frame();
        assert!(title_frame.origin.x + title_frame.size.width <= selector_frame.origin.x);
        assert!(selector_frame.origin.x + selector_frame.size.width <= copy_frame.origin.x);
        assert_eq!(
            card.heading_button.toolTip().unwrap().to_string(),
            "展开译文"
        );
        assert!(
            !card.selector_service_ids.is_empty(),
            "card must expose at least its current service in the chooser"
        );
        assert_eq!(card.selector.tag(), index as isize);
    }
    let status_frame = app.status_label.frame();
    let pin_frame = app.pin_button.frame();
    for toggle in &app.translation_toggles {
        let frame = toggle.frame();
        assert!(
            frame.origin.x + frame.size.width <= pin_frame.origin.x,
            "service toggles must not collide with pin action: toggle={frame:?}, pin={pin_frame:?}"
        );
    }
    for card in &app.translation_cards {
        if card.container.isHidden() {
            continue;
        }
        let container_height = card.container.frame().size.height;
        let container_frame = card.container.frame();
        assert!(
            container_frame.origin.y >= status_frame.origin.y + status_frame.size.height,
            "translation cards must stay above the status footer: card={container_frame:?}, status={status_frame:?}"
        );
        for button in [&card.heading_button, &card.copy_button] {
            let frame = button.frame();
            assert!(
                frame.origin.y >= 0. && frame.origin.y + frame.size.height <= container_height + 1.,
                "card action button must stay inside its container: button={frame:?}, container_height={container_height}"
            );
        }
    }
    let expanded_text = "expanded card layout check\n".repeat(80);
    app.translation_cards[0]
        .output
        .setString(&text(&expanded_text));
    app.toggle_result(0);
    assert!(!app.translation_cards[0].collapsed);
    assert_eq!(
        app.translation_cards[0]
            .heading_button
            .toolTip()
            .unwrap()
            .to_string(),
        "收起译文"
    );
    let expanded_frame = app.translation_cards[0].container.frame();
    assert!(expanded_frame.size.height > TRANSLATION_CARD_HEADER_HEIGHT);
    let expanded_output_frame = app.translation_cards[0].output_scroll.frame();
    assert!(
        expanded_output_frame.origin.y >= 0.
            && expanded_output_frame.origin.y + expanded_output_frame.size.height
                <= expanded_frame.size.height + 1.,
        "expanded output must stay inside its card: output={expanded_output_frame:?}, card={expanded_frame:?}"
    );
    let output_document_height = app.translation_cards[0]
        .output_scroll
        .documentView()
        .expect("translation document view")
        .frame()
        .size
        .height;
    assert!(
        output_document_height > expanded_output_frame.size.height,
        "long translation must have a document taller than its scroll viewport: document={output_document_height}, viewport={}",
        expanded_output_frame.size.height
    );
    assert!(
        app.translation_cards[0]
            .output_scroll
            .documentVisibleRect()
            .size
            .height
            < output_document_height,
        "long translation must have a nonzero vertical scroll range"
    );
    let output_clip = app.translation_cards[0].output_scroll.contentView();
    let output_scroll_start = app.translation_cards[0]
        .output_scroll
        .documentVisibleRect()
        .origin
        .y;
    output_clip.scrollToPoint(NSPoint::new(
        0.,
        output_document_height - expanded_output_frame.size.height,
    ));
    app.translation_cards[0]
        .output_scroll
        .reflectScrolledClipView(&output_clip);
    assert!(
        app.translation_cards[0]
            .output_scroll
            .documentVisibleRect()
            .origin
            .y
            > output_scroll_start,
        "long translation must scroll to lower content"
    );
    output_clip.scrollToPoint(NSPoint::new(0., 0.));
    app.translation_cards[0]
        .output_scroll
        .reflectScrolledClipView(&output_clip);
    app.toggle_result(0);
    assert!(app.translation_cards[0].collapsed);
    app.translation_cards[0].output.setString(&text(""));
    app.relayout();
    for card in &mut app.translation_cards {
        card.output.setString(&text(&expanded_text));
        card.collapsed = false;
    }
    app.relayout();
    assert!(app.panel.frame().size.height <= 860.);
    let constrained_status = app.status_label.frame();
    for card in &app.translation_cards {
        let frame = card.container.frame();
        assert!(
            frame.origin.y >= constrained_status.origin.y + constrained_status.size.height,
            "constrained cards must stay above the status footer: card={frame:?}, status={constrained_status:?}"
        );
    }
    for card in &mut app.translation_cards {
        card.output.setString(&text(""));
        card.collapsed = true;
    }
    app.relayout();
    let initial_height = app.panel.frame().size.height;
    let dynamic_text = "动态高度测试\n".repeat(12);
    app.input.setString(&text(&dynamic_text));
    app.observe_input();
    assert!(app.panel.frame().size.height >= initial_height);
    let input_document_height = app
        .input_scroll
        .documentView()
        .expect("input document view")
        .frame()
        .size
        .height;
    assert!(
        input_document_height > app.input_scroll.frame().size.height,
        "long input must remain available in a scrollable document: document={input_document_height}, viewport={}",
        app.input_scroll.frame().size.height
    );
    assert!(
        app.input_scroll.documentVisibleRect().size.height < input_document_height,
        "long source text must have a nonzero vertical scroll range"
    );
    let input_clip = app.input_scroll.contentView();
    let input_scroll_start = app.input_scroll.documentVisibleRect().origin.y;
    input_clip.scrollToPoint(NSPoint::new(
        0.,
        input_document_height - app.input_scroll.frame().size.height,
    ));
    app.input_scroll.reflectScrolledClipView(&input_clip);
    assert!(
        app.input_scroll.documentVisibleRect().origin.y > input_scroll_start,
        "long source text must scroll to lower content"
    );
    input_clip.scrollToPoint(NSPoint::new(0., 0.));
    app.input_scroll.reflectScrolledClipView(&input_clip);
    app.clear();
    for field in [&app.input, &app.output] {
        let scroll = field.enclosingScrollView().expect("text area scroll view");
        assert_eq!(scroll.borderType(), NSBorderType::LineBorder);
        assert!(scroll.drawsBackground());
        assert!(field.drawsBackground());
    }
    let has_clear_action: bool =
        unsafe { msg_send![&*app._actions, respondsToSelector: sel!(clear:)] };
    assert!(has_clear_action, "clear button needs a target action");
    app.output.setString(&text("Copy this text"));
    assert!(app.panel.makeFirstResponder(Some(&app.output)));
    app.output
        .setSelectedRange(objc2_foundation::NSRange::new(0, 4));
    assert_eq!(NSTextInputClient::selectedRange(&*app.output).length, 4);
    let test_pasteboard = NSPasteboard::pasteboardWithUniqueName();
    let plain_text = text("NSStringPboardType");
    assert!(
        app.output
            .writeSelectionToPasteboard_type(&test_pasteboard, &plain_text),
        "selected text must be exportable as plain text"
    );
    assert_eq!(
        test_pasteboard
            .stringForType(&plain_text)
            .expect("selected translation text")
            .to_string(),
        "Copy"
    );
    app.input.setString(&text("请不要现在付款。"));
    app.observe_input();
    assert_eq!(app.draft.current.as_ref().unwrap().text, "请不要现在付款。");
    let generation = app.draft.generation;
    assert!(app.draft.accept(generation, "Please don't pay now.".into()));
    app.set_preview_text("Please don't pay now.");
    app.copy_button.setEnabled(true);
    app.input.setString(&text("等我确认后再付。"));
    app.observe_input();
    assert!(app.draft.translation.is_none());
    assert!(!app.copy_button.isEnabled());
    assert!(app.output.string().is_empty());
    app.input.setString(&text("清空之前的原文"));
    app.observe_input();
    let stale_generation = app.draft.generation;
    assert!(app.draft.accept(stale_generation, "Old result".into()));
    app.set_preview_text("Old result");
    app.copy_button.setEnabled(true);
    app.clear();
    assert!(app.input.string().is_empty());
    assert!(app.output.string().is_empty());
    assert!(app.draft.current.is_none());
    assert!(app.draft.translation.is_none());
    assert!(!app.copy_button.isEnabled());
    assert!(!app.draft.accept(stale_generation, "Late result".into()));
    assert!(
        app.config
            .services
            .iter()
            .any(|service| { service.kind == easytranslate::service::ServiceKind::Google })
    );
    assert!(
        app.config
            .services
            .iter()
            .any(|service| { service.kind == easytranslate::service::ServiceKind::OpenAiCli })
    );
    app.set_mode(true);
    assert_eq!(app.language(), "zh-Hans");
    assert_eq!(app.source_language(), "auto");
    app.set_mode(false);
    assert_eq!(app.language(), "en");
    assert_eq!(app.source_language(), "zh-Hans");
    easytranslate::service::sync_ollama_models(
        &mut app.config.services,
        &["qwen2.5:0.5b-instruct".into(), "qwen2.5:3b-instruct".into()],
    );
    app.refresh_provider_controls();
    app.config.active_service = app
        .config
        .services
        .iter()
        .position(|service| service.kind == easytranslate::service::ServiceKind::OpenAiCli)
        .expect("default Codex service");
    app.config.provider = easytranslate::translation::Provider::Llm;
    app.settings();
    assert!(app.settings_window.is_some());
    let settings_window_frame = app
        .settings_window
        .as_ref()
        .expect("settings window")
        .frame();
    assert_eq!(settings_window_frame.size.width, 560.);
    assert!(app.siliconflow_endpoint_field.is_some());
    assert!(app.siliconflow_model_field.is_some());
    assert!(app.siliconflow_api_key_field.is_some());
    assert!(app.codex_model_field.is_some());
    assert!(app.ollama_model_field.is_some());
    assert!(app.ollama_pull_button.is_some());
    assert!(app.ollama_cancel_button.is_some());
    assert!(app.libretranslate_endpoint_field.is_some());
    assert!(app.libretranslate_api_key_field.is_some());
    assert!(app.libretranslate_save_button.is_some());
    let active_before_libretranslate = app.config.active_service;
    app.config.active_service = app
        .config
        .services
        .iter()
        .position(|service| service.kind == easytranslate::service::ServiceKind::LibreTranslate)
        .expect("LibreTranslate service");
    app.refresh_provider_controls();
    assert!(
        !app.libretranslate_endpoint_field
            .as_ref()
            .unwrap()
            .isHidden()
    );
    assert!(!app.libretranslate_save_button.as_ref().unwrap().isHidden());
    app.config.active_service = active_before_libretranslate;
    app.refresh_provider_controls();
    assert!(app.siliconflow_endpoint_field.as_ref().unwrap().isHidden());
    assert!(
        app.libretranslate_endpoint_field
            .as_ref()
            .unwrap()
            .isHidden()
    );
    assert!(!app.codex_model_label.as_ref().unwrap().isHidden());
    assert!(!app.codex_model_note.as_ref().unwrap().isHidden());
    assert!(app.ollama_progress.is_some());
    assert!(app.ollama_delete_button.is_some());
    assert!(app.ollama_browse_button.is_some());
    let service_picker_frame = app
        .provider_picker
        .as_ref()
        .expect("settings service picker")
        .frame();
    let ollama_field_frame = app
        .ollama_model_field
        .as_ref()
        .expect("settings Ollama model field")
        .frame();
    assert!(
        service_picker_frame.origin.y
            > ollama_field_frame.origin.y + ollama_field_frame.size.height,
        "settings service section must not overlap Ollama controls"
    );
    assert!(service_picker_frame.size.width >= 480.);
    let ollama_pull_frame = app
        .ollama_pull_button
        .as_ref()
        .expect("Ollama pull button")
        .frame();
    let ollama_model_frame = app.ollama_model_field.as_ref().unwrap().frame();
    assert!(
        ollama_model_frame.origin.x + ollama_model_frame.size.width <= ollama_pull_frame.origin.x
    );
    let shortcut_save_frame = app
        .record_buttons
        .first()
        .expect("shortcut record button")
        .frame();
    let shortcut_input_frame = app.shortcut_fields.first().expect("shortcut input").frame();
    assert!(
        shortcut_input_frame.origin.x + shortcut_input_frame.size.width
            <= shortcut_save_frame.origin.x
    );
    assert!(
        app.translation_cards.iter().all(|card| card
            .selector_service_ids
            .iter()
            .any(|id| id.starts_with("ollama:"))),
        "new Ollama models must be available from every card's service chooser"
    );
    assert!(
        app.ollama_refresh_button
            .as_ref()
            .expect("Ollama refresh button")
            .isHidden(),
        "Ollama management must be hidden while Codex is selected"
    );
    assert!(
        app.ollama_section_label
            .as_ref()
            .expect("Ollama section label")
            .isHidden(),
        "Ollama section title must be hidden while Codex is selected"
    );
    assert!(
        app.ollama_model_label
            .as_ref()
            .expect("Ollama model label")
            .isHidden()
    );
    assert!(
        app.ollama_model_field
            .as_ref()
            .expect("Ollama model field")
            .isHidden()
    );
    assert!(
        app.ollama_pull_button
            .as_ref()
            .expect("Ollama pull button")
            .isHidden()
    );
    assert!(
        app.ollama_delete_button
            .as_ref()
            .expect("Ollama delete button")
            .isHidden()
    );
    assert!(
        app.ollama_browse_button
            .as_ref()
            .expect("Ollama browse button")
            .isHidden()
    );
    let codex_controls_hidden: bool = unsafe {
        msg_send![
            &**app.codex_model_field.as_ref().expect("Codex model picker"),
            isHidden
        ]
    };
    assert!(
        !codex_controls_hidden,
        "Codex settings follow the active Codex service"
    );
    let codex_model_count: isize = unsafe {
        msg_send![
            &**app.codex_model_field.as_ref().expect("Codex model picker"),
            numberOfItems
        ]
    };
    assert!(codex_model_count >= 2, "Codex model picker needs presets");
    let ollama_service_index = app
        .config
        .services
        .iter()
        .position(|service| service.kind == easytranslate::service::ServiceKind::Ollama)
        .expect("installed Ollama service");
    app.config.active_service = ollama_service_index;
    app.refresh_provider_controls();
    assert!(!app.ollama_section_label.as_ref().unwrap().isHidden());
    assert!(!app.ollama_model_field.as_ref().unwrap().isHidden());
    assert!(app.codex_model_field.as_ref().unwrap().isHidden());
    assert!(app.codex_model_label.as_ref().unwrap().isHidden());
    assert!(app.codex_model_note.as_ref().unwrap().isHidden());
    assert!(app.siliconflow_endpoint_field.as_ref().unwrap().isHidden());
    assert!(
        app.libretranslate_endpoint_field
            .as_ref()
            .unwrap()
            .isHidden()
    );
    let libretranslate_service_index = app
        .config
        .services
        .iter()
        .position(|service| service.kind == easytranslate::service::ServiceKind::LibreTranslate)
        .expect("LibreTranslate service");
    app.config.active_service = libretranslate_service_index;
    app.refresh_provider_controls();
    assert!(
        !app.libretranslate_endpoint_field
            .as_ref()
            .unwrap()
            .isHidden()
    );
    assert!(!app.libretranslate_labels[0].isHidden());
    assert!(app.ollama_section_label.as_ref().unwrap().isHidden());
    assert!(app.codex_model_label.as_ref().unwrap().isHidden());
    assert!(app.siliconflow_endpoint_field.as_ref().unwrap().isHidden());
    app.config.active_service = active_before_libretranslate;
    app.refresh_provider_controls();
    let settings_provider_count: isize = unsafe {
        msg_send![
            &**app
                .provider_picker
                .as_ref()
                .expect("settings provider picker"),
            numberOfItems
        ]
    };
    assert_eq!(settings_provider_count as usize, app.config.services.len());
    if let Some(window) = &app.settings_window {
        window.orderOut(None);
    }
    assert!(!app.panel.isVisible());
    println!(
        "PASS: AppKit text areas have borders and export selected text; clear invalidates stale translation without showing a window"
    );
}
pub fn online_smoke() {
    let mtm = MainThreadMarker::new().expect("UI self-check must run on the main thread");
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let mut app = App::new(mtm);
    app.config.provider = easytranslate::translation::Provider::Llm;
    app.config.online_enabled = true;
    unsafe {
        let _: () = msg_send![&*app.source, selectItemAtIndex: 2isize];
        let _: () = msg_send![&*app.target, selectItemAtIndex: 0isize];
    }
    app.input
        .setString(&text("请不要现在付款，等我确认后再付。"));
    app.observe_input();
    app.changed = Instant::now() - Duration::from_secs(1);
    let generation = app.draft.generation;
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.draft.translation.is_none() && Instant::now() < deadline {
        app.tick();
        if app.attempted == Some(generation) && !app.busy {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let result = app
        .draft
        .translation
        .as_ref()
        .unwrap_or_else(|| panic!("online UI translation failed: {}", app.output.string()));
    let lower = result.to_lowercase();
    assert!(lower.contains("pay") && lower.contains("confirm"));
    assert!(!lower.contains("i will pay"));
    assert!(app.copy_button.isEnabled());
    assert!(!app.panel.isVisible());
    println!(
        "PASS: AppKit input -> automatic online translation -> editable result and copy enabled: {result}"
    );
}
pub fn run() {
    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let mut app = App::new(mtm);
    if app.selected_comparison_uses(easytranslate::service::ServiceKind::LocalAi) {
        app.preload_local_model();
    }
    if app.selected_ollama_model().is_some() {
        app.preload_selected_ollama();
    }
    application.setDelegate(Some(ProtocolObject::from_ref(&*app._actions)));
    app.refresh_provider_controls();
    match hotkeys::Registry::new() {
        Ok(mut registry) => {
            if let Err(error) = registry.configure(app.config.shortcuts) {
                app.status(&error);
            }
            app.hotkeys = Some(registry);
        }
        Err(error) => app.status(&error),
    }
    let _timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.15,
            &app._actions,
            sel!(tick:),
            None,
            true,
        )
    };
    let handler = block2::RcBlock::new(|event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let mut consumed = false;
        with_app(|app| {
            let event = unsafe { event.as_ref() };
            consumed = app.recording_event(event) || app.copy_selection_event(event);
        });
        if consumed {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    let _monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
    };
    STATE.with(|s| *s.borrow_mut() = Some(app));
    with_app(|app| app.refresh_ollama_models());
    application.run();
}

#[cfg(test)]
mod hotkey_behavior_tests {
    use super::{
        selected_text_copy_first, shortcut_menu_title, should_preload_ollama, toggle_action,
    };
    use easytranslate::shortcuts::Shortcut;
    use std::time::{Duration, Instant};

    #[test]
    fn option_d_uses_copied_selection_before_accessibility_fallback() {
        let order = std::cell::RefCell::new(Vec::new());
        let result = selected_text_copy_first(
            || {
                order.borrow_mut().push("copy");
                Ok(" selected text ".to_owned())
            },
            || {
                order.borrow_mut().push("accessibility");
                Ok("different text".to_owned())
            },
        );

        assert_eq!(result.as_deref(), Ok(" selected text "));
        assert_eq!(*order.borrow(), ["copy"]);
    }

    #[test]
    fn option_d_falls_back_to_accessibility_when_copy_has_no_selection() {
        let order = std::cell::RefCell::new(Vec::new());
        let result = selected_text_copy_first(
            || {
                order.borrow_mut().push("copy");
                Err("no clipboard update".to_owned())
            },
            || {
                order.borrow_mut().push("accessibility");
                Ok("selected through accessibility".to_owned())
            },
        );

        assert_eq!(result.as_deref(), Ok("selected through accessibility"));
        assert_eq!(*order.borrow(), ["copy", "accessibility"]);
    }

    #[test]
    fn option_a_reopens_an_obscured_panel_and_only_closes_it_when_frontmost() {
        assert!(toggle_action(false, false, false), "hidden panel opens");
        assert!(
            toggle_action(true, false, true),
            "a visible panel obscured by another app should be raised, not closed"
        );
        assert!(
            toggle_action(true, true, false),
            "a different window in EasyTranslate should raise the panel"
        );
        assert!(!toggle_action(true, true, true), "frontmost panel closes");
    }

    #[test]
    fn menu_titles_show_shortcuts_with_macos_symbols() {
        assert_eq!(
            shortcut_menu_title("开关翻译框", Shortcut::parse("Option+A").unwrap()),
            "开关翻译框（⌥A）"
        );
        assert_eq!(
            shortcut_menu_title(
                "截图翻译",
                Shortcut::parse("Command+Option+Shift+S").unwrap()
            ),
            "截图翻译（⌥⇧⌘S）"
        );
    }

    #[test]
    fn ollama_preload_runs_once_for_the_selected_model_even_with_existing_text() {
        let now = Instant::now();
        assert!(should_preload_ollama(
            true,
            "translategemma:latest",
            true,
            None,
            None,
            now,
        ));
        assert!(should_preload_ollama(
            true,
            "translategemma:latest",
            false,
            None,
            None,
            now,
        ));
        assert!(!should_preload_ollama(
            true,
            "translategemma:latest",
            true,
            Some("translategemma:latest"),
            None,
            now,
        ));
        assert!(!should_preload_ollama(
            true,
            "translategemma:latest",
            true,
            None,
            Some(("translategemma:latest", now - Duration::from_secs(60))),
            now,
        ));
        assert!(!should_preload_ollama(
            true,
            "translategemma:latest",
            true,
            None,
            Some(("translategemma:latest", now - Duration::from_secs(5 * 60))),
            now,
        ));
        assert!(should_preload_ollama(
            true,
            "qwen2.5:3b-instruct",
            true,
            None,
            Some(("translategemma:latest", now)),
            now,
        ));
        assert!(!should_preload_ollama(
            false,
            "translategemma:latest",
            true,
            None,
            None,
            now,
        ));
    }
}
