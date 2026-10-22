import AppKit
import ApplicationServices
let translator=AXUIElementCreateApplication(Int32(CommandLine.arguments[1])!)
func attribute(_ el:AXUIElement,_ name:String)->CFTypeRef? {
    var v:CFTypeRef?; return AXUIElementCopyAttributeValue(el,name as CFString,&v) == .success ? v : nil
}
func all(_ el:AXUIElement,_ depth:Int=0)->[AXUIElement] {
    if depth>12 {return []}
    return [el]+(attribute(el,"AXChildren") as? [AXUIElement] ?? []).flatMap{all($0,depth+1)}
}
func nodes()->[AXUIElement] { all(translator) }
func press(_ title:String) {
    guard let control=nodes().first(where:{
        let role = attribute($0,"AXRole") as? String
        return (role == "AXButton" || role == "AXMenuItem") && attribute($0,"AXTitle") as? String == title
    }) else {fatalError("Missing control: "+title)}
    precondition(AXUIElementPerformAction(control,kAXPressAction as CFString) == .success)
    Thread.sleep(forTimeInterval:0.3)
}
func statusContains(_ text:String)->Bool {nodes().contains{(attribute($0,"AXValue") as? String ?? "").contains(text)}}
press("设置")
let fields=nodes().filter{attribute($0,"AXRole") as? String == "AXTextField"}
precondition(fields.count>=3,"Missing shortcut fields")
for title in ["刷新 Ollama 模型","下载并添加","浏览 Ollama 模型库"] {
    precondition(nodes().contains{
        (attribute($0,"AXRole") as? String == "AXButton") &&
        (attribute($0,"AXTitle") as? String == title)
    },"Ollama management control is missing while Codex is selected: "+title)
}
func set(_ i:Int,_ value:String) {precondition(AXUIElementSetAttributeValue(fields[i],kAXValueAttribute as CFString,value as CFString) == .success)}
set(0,"Option+A");set(1,"Option+A");press("保存快捷键")
precondition(statusContains("不能使用同一快捷键"),"Duplicates were accepted")
set(0,"Control+Shift+Q");set(1,"Option+D");press("保存快捷键")
precondition(statusContains("已保存并立即生效"),"Custom keys were not saved")
let url=FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/EasyTranslate/config.json")
let json=try! JSONSerialization.jsonObject(with:Data(contentsOf:url)) as! [String:Any]
let keys=json["shortcuts"] as! [[String:Int]]
precondition(keys[0]["code"]==12 && keys[0]["modifiers"]==4608,"Wrong persisted binding")
print("PASS: settings window rejects duplicate keys, saves custom modifiers immediately, persists config")
if CommandLine.arguments.count<3 {
    press("恢复默认");press("保存快捷键")
    precondition(statusContains("已保存并立即生效"))
}
for window in (attribute(translator,"AXWindows") as? [AXUIElement] ?? []) where (attribute(window,"AXTitle") as? String ?? "").contains("· 设置") {
    if let close=attribute(window,"AXCloseButton") { _=AXUIElementPerformAction(close as! AXUIElement,kAXPressAction as CFString) }
}
