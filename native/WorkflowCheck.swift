import AppKit
import ApplicationServices
let fixturePID=Int32(CommandLine.arguments[1])!
let translatorPID=Int32(CommandLine.arguments[2])!
func attribute(_ element:AXUIElement,_ name:String)->CFTypeRef? {
    var value:CFTypeRef?
    return AXUIElementCopyAttributeValue(element,name as CFString,&value) == .success ? value : nil
}
func elementAttribute(_ element:AXUIElement,_ name:String)->AXUIElement? {
    guard let value=attribute(element,name),CFGetTypeID(value)==AXUIElementGetTypeID() else { return nil }
    return (value as! AXUIElement)
}
func findButton(_ element:AXUIElement,depth:Int=0)->AXUIElement? {
    if depth > 12 { return nil }
    if attribute(element,"AXRole") as? String == "AXButton",attribute(element,"AXTitle") as? String == "复制译文" { return element }
    for child in (attribute(element,"AXChildren") as? [AXUIElement] ?? []) {
        if let result=findButton(child,depth:depth+1) { return result }
    }
    return nil
}
func textValues(_ el:AXUIElement,_ depth:Int=0)->[String] {
    if depth>12 {return []}
    let own=attribute(el,"AXRole") as? String == "AXTextArea" ? [attribute(el,"AXValue") as? String ?? ""] : []
    return own+(attribute(el,"AXChildren") as? [AXUIElement] ?? []).flatMap{textValues($0,depth+1)}
}
func textAreas(_ el:AXUIElement,_ depth:Int=0)->[AXUIElement] {
    if depth>12 {return []}
    let own=(attribute(el,"AXRole") as? String == "AXTextArea") ? [el] : []
    return own+(attribute(el,"AXChildren") as? [AXUIElement] ?? []).flatMap{textAreas($0,depth+1)}
}
func findTitle(_ el:AXUIElement,_ title:String,_ depth:Int=0)->AXUIElement? {
    if depth>12 {return nil}
    if attribute(el,"AXTitle") as? String == title {return el}
    for child in (attribute(el,"AXChildren") as? [AXUIElement] ?? []) {
        if let result=findTitle(child,title,depth+1) {return result}
    }
    return nil
}
let fixture=AXUIElementCreateApplication(fixturePID)
let translator=AXUIElementCreateApplication(translatorPID)
NSRunningApplication(processIdentifier:fixturePID)?.activate(options:[.activateIgnoringOtherApps])
Thread.sleep(forTimeInterval:0.2)
let staticMode=CommandLine.arguments.contains("static")
let fixtureText=staticMode ? "Hello, see you tomorrow." : "This is an automatic test draft, do not send."
let selectionText="Hello, see you tomorrow."
let selectedText=staticMode ? fixtureText : selectionText
func sendHotkey(_ key:CGKeyCode,_ flags:CGEventFlags) {
    let down=CGEvent(keyboardEventSource:nil,virtualKey:key,keyDown:true)!
    down.flags=flags;down.post(tap:.cghidEventTap)
    let up=CGEvent(keyboardEventSource:nil,virtualKey:key,keyDown:false)!
    up.flags=flags;up.post(tap:.cghidEventTap)
}
var editor:AXUIElement?
for _ in 0..<50 {
    let candidates = staticMode ? textAreas(fixture) : elementAttribute(fixture,"AXFocusedUIElement").map { [$0] } ?? []
    if let candidate=candidates.first(where: { attribute($0,"AXValue") as? String == fixtureText }) {
        editor=candidate
        break
    }
    Thread.sleep(forTimeInterval:0.1)
}
guard let editor else { fatalError("Dedicated fixture not ready") }
var initialRange=CFRange(location:0,length:(attribute(editor,"AXValue") as! String as NSString).length)
let initialSelection=AXValueCreate(.cfRange,&initialRange)!
precondition(AXUIElementSetAttributeValue(editor,kAXSelectedTextRangeAttribute as CFString,initialSelection) == .success)
if staticMode,let window=elementAttribute(fixture,"AXFocusedWindow"),let button=findTitle(window,"Focus elsewhere") {
    _ = AXUIElementPerformAction(button,kAXPressAction as CFString)
}
NSRunningApplication(processIdentifier:fixturePID)?.activate(options:[])
for _ in 0..<30 {
    if NSWorkspace.shared.frontmostApplication?.processIdentifier == fixturePID {break}
    Thread.sleep(forTimeInterval:0.1)
}
guard NSWorkspace.shared.frontmostApplication?.processIdentifier == fixturePID else { fatalError("Refusing hotkey outside fixture") }
let custom=CommandLine.arguments.contains("custom")
sendHotkey(custom ? 12 : 0,custom ? [.maskControl,.maskShift] : [.maskAlternate])
var visibleAfterToggle=false
for _ in 0..<100 {
    visibleAfterToggle=(attribute(translator,"AXWindows") as? [AXUIElement] ?? []).contains {
        (attribute($0,"AXRole") as? String) == "AXWindow" && attribute($0,"AXTitle") as? String == "EasyTranslate · 快速翻译"
    }
    if visibleAfterToggle { break }
    Thread.sleep(forTimeInterval:0.1)
}
precondition(visibleAfterToggle,"Alt+A did not open the translation panel")
func dump(_ el:AXUIElement,_ depth:Int=0) {
    if depth>8 { return }
    print(String(repeating:" ",count:depth),attribute(el,"AXRole") ?? "",attribute(el,"AXTitle") ?? "",attribute(el,"AXDescription") ?? "",attribute(el,"AXValue") ?? "",attribute(el,"AXEnabled") ?? "")
    for child in (attribute(el,"AXChildren") as? [AXUIElement] ?? []) { dump(child,depth+1) }
}
precondition(attribute(translator,"AXFocusedWindow") != nil,"Alt+A did not focus the translation panel")
print("PASS: Alt+A opens the translation panel")

if !staticMode {
    precondition(AXUIElementSetAttributeValue(editor,kAXValueAttribute as CFString,selectionText as CFString) == .success)
}
var range=CFRange(location:0,length:(selectedText as NSString).length)
let selection=AXValueCreate(.cfRange,&range)!
precondition(AXUIElementSetAttributeValue(editor,kAXSelectedTextRangeAttribute as CFString,selection) == .success)
NSRunningApplication(processIdentifier:fixturePID)?.activate(options:[])
Thread.sleep(forTimeInterval:0.3)
precondition(AXUIElementSetAttributeValue(editor,kAXSelectedTextRangeAttribute as CFString,selection) == .success)
Thread.sleep(forTimeInterval:0.2)
for _ in 0..<30 {
    if NSWorkspace.shared.frontmostApplication?.processIdentifier == fixturePID {break}
    Thread.sleep(forTimeInterval:0.1)
}
precondition(NSWorkspace.shared.frontmostApplication?.processIdentifier == fixturePID,"Fixture did not regain focus before Alt+D")
sendHotkey(2,[.maskAlternate])
var button:AXUIElement?
for _ in 0..<100 {
    for window in (attribute(translator,"AXWindows") as? [AXUIElement] ?? []) {
        if let candidate=findButton(window),attribute(candidate,"AXEnabled") as? Bool == true { button=candidate;break }
    }
    if button != nil { break }
    Thread.sleep(forTimeInterval:0.1)
}
guard let button else {
    let currentInputs=(attribute(translator,"AXWindows") as? [AXUIElement] ?? []).flatMap{textAreas($0)}.compactMap{attribute($0,"AXValue") as? String}
    print("INPUTS AFTER ALT-D: " + currentInputs.joined(separator: " | ") + " CLIPBOARD: " + (NSPasteboard.general.string(forType: .string) ?? "<empty>"))
    for window in (attribute(translator,"AXWindows") as? [AXUIElement] ?? []) { dump(window) }
    fflush(stdout)
    fatalError("Translation replacement button did not become enabled")
}
var selectedTranslated=false
for _ in 0..<100 {
    let windows=attribute(translator,"AXWindows") as? [AXUIElement] ?? []
    let values=windows.flatMap{textValues($0)}
    let input=windows.flatMap{textAreas($0)}.first.flatMap{attribute($0,"AXValue") as? String} ?? ""
    if input == selectionText && values.contains(where:{$0.contains("明天")}) {selectedTranslated=true;break}
    Thread.sleep(forTimeInterval:0.1)
}
if !selectedTranslated {
    let values=(attribute(translator,"AXWindows") as? [AXUIElement] ?? []).flatMap{textValues($0)}
    print("VALUES AFTER ALT-D: " + values.joined(separator: " | "))
    for window in (attribute(translator,"AXWindows") as? [AXUIElement] ?? []) { dump(window) }
    fflush(stdout)
}
precondition(selectedTranslated,"Alt+D did not put selected text in the page and translate it")
precondition(attribute(editor,"AXValue") as? String == selectedText,"Selection translation modified source")
print("PASS: Alt+D opens the page, translates selected text, and leaves the source unchanged")
if !CommandLine.arguments.contains("screenshot") { exit(0) }
precondition(AXUIElementSetAttributeValue(editor,kAXValueAttribute as CFString,"Please confirm invoice number 7351." as CFString) == .success)
NSRunningApplication(processIdentifier:fixturePID)?.activate(options:[])
Thread.sleep(forTimeInterval:0.5)
precondition(NSWorkspace.shared.frontmostApplication?.processIdentifier == fixturePID)
var position=CGPoint.zero
var size=CGSize.zero
precondition(AXValueGetValue(attribute(editor,"AXPosition") as! AXValue,.cgPoint,&position))
precondition(AXValueGetValue(attribute(editor,"AXSize") as! AXValue,.cgSize,&size))
let captureDown=CGEvent(keyboardEventSource:nil,virtualKey:1,keyDown:true)!
captureDown.flags=[.maskCommand,.maskAlternate];captureDown.post(tap:.cghidEventTap)
let captureUp=CGEvent(keyboardEventSource:nil,virtualKey:1,keyDown:false)!
captureUp.flags=captureDown.flags;captureUp.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:1.5)
let start=CGPoint(x:position.x+4,y:position.y+4)
let finish=CGPoint(x:position.x+size.width-4,y:position.y+size.height-4)
CGEvent(mouseEventSource:nil,mouseType:.mouseMoved,mouseCursorPosition:start,mouseButton:.left)!.post(tap:.cghidEventTap)
CGEvent(mouseEventSource:nil,mouseType:.leftMouseDown,mouseCursorPosition:start,mouseButton:.left)!.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:0.15)
for step in 1...15 {
    let fraction=CGFloat(step)/15
    let point=CGPoint(x:start.x+(finish.x-start.x)*fraction,y:start.y+(finish.y-start.y)*fraction)
    CGEvent(mouseEventSource:nil,mouseType:.leftMouseDragged,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
    Thread.sleep(forTimeInterval:0.04)
}
Thread.sleep(forTimeInterval:0.15)
CGEvent(mouseEventSource:nil,mouseType:.leftMouseUp,mouseCursorPosition:finish,mouseButton:.left)!.post(tap:.cghidEventTap)
var screenshotTranslated=false
for _ in 0..<600 {
    let values=(attribute(translator,"AXWindows") as? [AXUIElement] ?? []).flatMap{textValues($0)}
    if values.contains(where:{$0.contains("7351") && $0.contains("发票")}) {screenshotTranslated=true;break}
    Thread.sleep(forTimeInterval:0.1)
}
precondition(screenshotTranslated,"Screenshot hotkey/capture/OCR/translation did not complete")
precondition(attribute(editor,"AXValue") as? String == "Please confirm invoice number 7351.")
print("PASS: screenshot hotkey → drag fixture region → OCR → translation; source unchanged")
