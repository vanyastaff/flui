// An assistive-technology client for `scripts/check-macos-a11y.py`.
//
// Given a pid, reads that process's accessibility tree through the same
// `AXUIElement` API VoiceOver uses, prints it, finds the first element whose
// role is AXButton and whose title or description is the requested label,
// performs AXPress on it, and reports whether a static text with the
// expected value is present afterwards. Nothing here knows FLUI; it is what
// any screen reader sees.
//
// Usage: macos-ax-client <pid> <button-label> <expected-value-after-press>
// Exit 0 when the button was found, pressed, and the value appeared;
// 1 otherwise; 2 when this process is not trusted for accessibility.

import ApplicationServices
import Foundation

func attribute(_ element: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    let status = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return status == .success ? value : nil
}

func string(_ element: AXUIElement, _ name: String) -> String? {
    guard let value = attribute(element, name) else { return nil }
    if let text = value as? String { return text }
    if let number = value as? NSNumber { return number.stringValue }
    return nil
}

func children(_ element: AXUIElement) -> [AXUIElement] {
    guard let raw = attribute(element, kAXChildrenAttribute) as? [AXUIElement] else { return [] }
    return raw
}

struct Node {
    let element: AXUIElement
    let role: String
    let title: String?
    let description: String?
    let value: String?
    let depth: Int
}

func walk(_ element: AXUIElement, depth: Int, into nodes: inout [Node]) {
    let role = string(element, kAXRoleAttribute) ?? "?"
    nodes.append(Node(
        element: element,
        role: role,
        title: string(element, kAXTitleAttribute),
        description: string(element, kAXDescriptionAttribute),
        value: string(element, kAXValueAttribute),
        depth: depth))
    for child in children(element) {
        walk(child, depth: depth + 1, into: &nodes)
    }
}

func dump(_ nodes: [Node]) {
    for node in nodes {
        let indent = String(repeating: "  ", count: node.depth)
        var parts = [node.role]
        if let title = node.title, !title.isEmpty { parts.append("title=\"\(title)\"") }
        if let description = node.description, !description.isEmpty { parts.append("description=\"\(description)\"") }
        if let value = node.value, !value.isEmpty { parts.append("value=\"\(value)\"") }
        print("AX \(indent)\(parts.joined(separator: " "))")
    }
}

func tree(of pid: pid_t) -> [Node] {
    let app = AXUIElementCreateApplication(pid)
    var nodes: [Node] = []
    guard let windows = attribute(app, kAXWindowsAttribute) as? [AXUIElement] else {
        return nodes
    }
    for window in windows {
        walk(window, depth: 0, into: &nodes)
    }
    return nodes
}

let arguments = CommandLine.arguments
guard arguments.count == 4, let pid = pid_t(arguments[1]) else {
    FileHandle.standardError.write("usage: macos-ax-client <pid> <button-label> <expected-value>\n".data(using: .utf8)!)
    exit(1)
}
let label = arguments[2]
let expected = arguments[3]

guard AXIsProcessTrusted() else {
    print("AX_CLIENT_RESULT=CANNOT_VERIFY (this process is not trusted for accessibility; grant it under System Settings > Privacy & Security > Accessibility)")
    exit(2)
}

// The first query is what activates the adapter (NSAccessibility has no
// attach event: the tree is assembled on the frame after a client first
// asks), and the window's own title-bar buttons are AXButtons too — so the
// retry waits for the labelled button, not for any button.
func isTarget(_ node: Node) -> Bool {
    node.role == "AXButton" && (node.title == label || node.description == label)
}
var before: [Node] = []
for _ in 0..<40 {
    before = tree(of: pid)
    if before.contains(where: isTarget) { break }
    Thread.sleep(forTimeInterval: 0.25)
}
print("AX_TREE_BEFORE nodes=\(before.count)")
dump(before)

guard let button = before.first(where: isTarget) else {
    print("AX_CLIENT_RESULT=FAIL (no AXButton labelled \"\(label)\" in the tree above)")
    exit(1)
}
let status = AXUIElementPerformAction(button.element, kAXPressAction as CFString)
print("AX_PRESS status=\(status.rawValue)")
guard status == .success else {
    print("AX_CLIENT_RESULT=FAIL (AXPress on the \"\(label)\" button returned \(status.rawValue))")
    exit(1)
}

var after: [Node] = []
var found = false
for _ in 0..<40 {
    after = tree(of: pid)
    found = after.contains(where: { $0.role == "AXStaticText" && ($0.value == expected || $0.title == expected || $0.description == expected) })
    if found { break }
    Thread.sleep(forTimeInterval: 0.25)
}
print("AX_TREE_AFTER nodes=\(after.count)")
dump(after)
if found {
    print("AX_CLIENT_RESULT=PASS (pressed \"\(label)\" through AXPress; a static text now reads \"\(expected)\")")
    exit(0)
} else {
    print("AX_CLIENT_RESULT=FAIL (after AXPress no static text reads \"\(expected)\")")
    exit(1)
}
