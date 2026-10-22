import AppKit
import ApplicationServices

guard CommandLine.arguments.count == 2, let pid = Int32(CommandLine.arguments[1]) else {
    fatalError("usage: inspect-ax-metadata.swift <pid>")
}

func attribute(_ element: AXUIElement, _ name: String) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return (status, value)
}

func string(_ element: AXUIElement, _ name: String) -> String {
    attribute(element, name).1 as? String ?? ""
}

func flag(_ element: AXUIElement, _ name: String) -> Bool {
    attribute(element, name).1 as? Bool ?? false
}

func settable(_ element: AXUIElement, _ name: String) -> Bool {
    var result = DarwinBoolean(false)
    return AXUIElementIsAttributeSettable(element, name as CFString, &result) == .success && result.boolValue
}

func bounds(_ element: AXUIElement) -> String {
    guard let position = attribute(element, "AXPosition").1 as? AXValue,
          let size = attribute(element, "AXSize").1 as? AXValue else { return "unknown" }
    var point = CGPoint.zero
    var dimensions = CGSize.zero
    guard AXValueGetValue(position, .cgPoint, &point), AXValueGetValue(size, .cgSize, &dimensions) else { return "unknown" }
    return "x=\(Int(point.x)) y=\(Int(point.y)) w=\(Int(dimensions.width)) h=\(Int(dimensions.height))"
}

func describe(_ element: AXUIElement) -> String {
    var owner = Int32(0)
    AXUIElementGetPid(element, &owner)
    return "pid=\(owner) role=\(string(element, "AXRole")) subrole=\(string(element, "AXSubrole")) focused=\(flag(element, "AXFocused")) valueSettable=\(settable(element, "AXValue")) selectedSettable=\(settable(element, "AXSelectedText")) \(bounds(element))"
}

let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 0.2)
let system = AXUIElementCreateSystemWide()
print("trusted=\(AXIsProcessTrusted()) frontmost=\(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0) target=\(pid)")
for (name, root) in [("app focus", app), ("system focus", system), ("focused window", app)] {
    let key = name == "focused window" ? "AXFocusedWindow" : "AXFocusedUIElement"
    let (status, value) = attribute(root, key)
    if let element = value as? AXUIElement {
        print("\(name) status=\(status.rawValue) \(describe(element))")
    } else {
        print("\(name) status=\(status.rawValue) unavailable")
    }
}

var visited = 0
func walk(_ element: AXUIElement, depth: Int, path: String) {
    visited += 1
    if visited > 500 || depth > 16 { return }
    let role = string(element, "AXRole")
    let nextPath = path.isEmpty ? role : "\(path)/\(role)"
    if ["AXTextField", "AXTextArea", "AXComboBox", "AXWebArea"].contains(role) {
        print("candidate depth=\(depth) path=\(nextPath) \(describe(element))")
    }
    for child in attribute(element, "AXChildren").1 as? [AXUIElement] ?? [] {
        walk(child, depth: depth + 1, path: nextPath)
    }
}

let (windowsStatus, windowsValue) = attribute(app, "AXWindows")
let windows = windowsValue as? [AXUIElement] ?? []
print("windows status=\(windowsStatus.rawValue) count=\(windows.count)")
for (index, window) in windows.prefix(5).enumerated() {
    print("window[\(index)] \(describe(window))")
    walk(window, depth: 0, path: "")
}
print("visited=\(visited)")
