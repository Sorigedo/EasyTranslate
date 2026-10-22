import AppKit
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 100,y: 200,width: 500,height: 200),styleMask: [.titled,.closable],backing: .buffered,defer: false)
window.title = "EasyTranslate disposable AX test"
window.isReleasedWhenClosed = false
let staticMode = CommandLine.arguments.contains("--static")
let field: NSView
let firstResponder: NSView
if staticMode {
    let view = NSTextView(frame: NSRect(x: 20,y: 40,width: 460,height: 100))
    view.string = "Hello, see you tomorrow."
    view.isEditable = false
    view.isSelectable = true
    field = view
    let button = NSButton(frame: NSRect(x: 20, y: 12, width: 140, height: 22))
    button.title = "Focus elsewhere"
    window.contentView?.addSubview(button)
    firstResponder = button
} else {
    let view = NSTextField(frame: NSRect(x: 20,y: 70,width: 460,height: 60))
    view.stringValue = "This is an automatic test draft, do not send."
    field = view
    firstResponder = view
}
window.contentView?.addSubview(field)
app.finishLaunching()
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(firstResponder)
app.activate(ignoringOtherApps: true)
let copyBlocker = staticMode ? NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
    let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    if modifiers.contains(.command) && event.keyCode == 8 { return nil }
    return event
} : nil
DispatchQueue.main.asyncAfter(deadline: .now()+0.3) { app.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); window.makeFirstResponder(firstResponder) }
app.run()
