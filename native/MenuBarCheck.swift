import AppKit
import ApplicationServices
let pid = pid_t(CommandLine.arguments[1])!
let app = NSRunningApplication(processIdentifier: pid)!
guard app.activationPolicy == .accessory else { print("FAIL: Dock entry must be absent"); exit(1) }
func attr(_ node:AXUIElement,_ key:String)->CFTypeRef? { var value:CFTypeRef?; return AXUIElementCopyAttributeValue(node,key as CFString,&value) == .success ? value : nil }
func all(_ node:AXUIElement)->[AXUIElement] { [node] + (attr(node,kAXChildrenAttribute) as? [AXUIElement] ?? []).flatMap(all) }
let root = AXUIElementCreateApplication(pid)
let nodes = all(root)
let items = nodes.filter { attr($0,kAXRoleAttribute) as? String == "AXMenuBarItem" && (attr($0,kAXTitleAttribute) as? String == "译" || attr($0,kAXDescriptionAttribute) as? String == "译" || (attr($0,kAXHelpAttribute) as? String ?? "").contains("EasyTranslate")) }
guard items.count == 1 else { print("FAIL: missing or duplicate status item"); exit(1) }
for title in ["快捷键设置…", "阅读 / 手动输入", "辅助功能权限", "退出"] {
 guard nodes.contains(where:{attr($0,kAXTitleAttribute) as? String == title}) else { print("FAIL: missing menu action",title); exit(1) }
}
print("PASS: accessory application, one status item, settings/translation/permission/quit actions. Physical visibility requires separate observation.")
