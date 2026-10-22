// Events target only EasyTranslate, never another application.
import AppKit
import ApplicationServices
let pid = pid_t(CommandLine.arguments[1])!
let app = AXUIElementCreateApplication(pid)
func read(_ node: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(node, key as CFString, &value) == .success ? value : nil
}
func walk(_ node: AXUIElement) -> [AXUIElement] {
    [node] + (read(node, kAXChildrenAttribute) as? [AXUIElement] ?? []).flatMap(walk)
}
func nodes() -> [AXUIElement] {
    (read(app, kAXWindowsAttribute) as? [AXUIElement] ?? []).flatMap(walk)
}
func require(_ condition: Bool, _ message: String) {
    if !condition { print("FAIL: " + message); exit(1) }
}
func buttons(_ title: String) -> [AXUIElement] {
    nodes().filter { read($0, kAXRoleAttribute) as? String == "AXButton" && read($0, kAXTitleAttribute) as? String == title }
}
func press(_ title: String) {
    guard let button = buttons(title).first else { require(false, "Missing button: " + title); return }
    require(AXUIElementPerformAction(button, kAXPressAction as CFString) == .success, "Cannot press " + title)
    Thread.sleep(forTimeInterval: 0.25)
}
func status(_ phrase: String) -> Bool { nodes().contains { (read($0, kAXValueAttribute) as? String ?? "").contains(phrase) } }
func key(_ code: CGKeyCode, _ flags: CGEventFlags) {
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)!
        event.flags = flags
        event.postToPid(pid)
    }
    Thread.sleep(forTimeInterval: 0.35)
}
require(buttons("录制").count == 3, "Three shortcut recorders must exist")
require(NSRunningApplication(processIdentifier: pid)?.activationPolicy == .accessory, "Expected menu-bar-only application")
if CommandLine.arguments.contains("--activate") {
    NSRunningApplication(processIdentifier: pid)?.activate(options: [])
    Thread.sleep(forTimeInterval: 0.3)
}
require(NSRunningApplication(processIdentifier: pid)?.isActive == true, "Open EasyTranslate settings before testing")
let fields = nodes().filter { read($0, kAXRoleAttribute) as? String == "AXTextField" }
require(fields.count == 3, "Missing shortcut fields")
let originals = fields.map { read($0, kAXValueAttribute) as? String ?? "" }
press("录制")
key(0, [])
require(status("至少包含"), "Unmodified typing must not be accepted")
key(53, [])
require(status("已取消录制"), "Escape must cancel")
require(read(fields[0], kAXValueAttribute) as? String == originals[0], "Cancel changed original shortcut")
press("录制")
key(12, [.maskControl, .maskShift])
require(read(fields[0], kAXValueAttribute) as? String == "Control+Shift+Q", "Actual keystroke did not record")
require(status("已录制"), "Recording did not finish")
for (field, value) in zip(fields, originals) {
    require(AXUIElementSetAttributeValue(field, kAXValueAttribute as CFString, value as CFString) == .success, "Cannot restore original field")
}
press("保存快捷键")
require(status("已保存并立即生效"), "Save failed")
press("录制")
press("取消")
require(status("已取消录制"), "Cancel button failed")
press("录制")
for window in read(app, kAXWindowsAttribute) as? [AXUIElement] ?? [] {
    if let close = read(window, kAXCloseButtonAttribute) {
        _ = AXUIElementPerformAction(close as! AXUIElement, kAXPressAction as CFString)
    }
}
Thread.sleep(forTimeInterval: 0.4)
let launch = Process()
launch.executableURL = URL(fileURLWithPath: "/usr/bin/open")
launch.arguments = [NSRunningApplication(processIdentifier: pid)!.bundleURL!.path]
try launch.run()
launch.waitUntilExit()
Thread.sleep(forTimeInterval: 0.6)
require(buttons("录制").count == 3, "Reopening must restore settings and stop recording")
print("PASS: menu-bar mode, recorders, invalid key, Escape, actual capture, save and cancel; original shortcuts preserved")
