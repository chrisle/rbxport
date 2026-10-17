// OS-level input for the macOS desktop suite. Never synthesizes DOM drops.
import AppKit
import ApplicationServices

func fail(_ message: String) -> Never { fputs(message + "\n", stderr); exit(1) }
func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success ? value : nil
}
func point(_ element: AXUIElement, _ key: String) -> CGPoint? {
    guard let raw = attribute(element, key), CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
    var result = CGPoint.zero
    return AXValueGetValue(unsafeBitCast(raw, to: AXValue.self), .cgPoint, &result) ? result : nil
}
func size(_ element: AXUIElement) -> CGSize? {
    guard let raw = attribute(element, kAXSizeAttribute), CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
    var result = CGSize.zero
    return AXValueGetValue(unsafeBitCast(raw, to: AXValue.self), .cgSize, &result) ? result : nil
}
func children(_ element: AXUIElement) -> [AXUIElement] { attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? [] }
func namesMatch(_ element: AXUIElement, _ names: [String], depth: Int = 0) -> Bool {
    if depth > 8 { return false }
    for key in [kAXTitleAttribute,kAXValueAttribute,kAXDescriptionAttribute,"AXFilename","AXURL"] {
        if let value = attribute(element,key) as? String,
           names.contains(where:{value.precomposedStringWithCanonicalMapping.contains($0.precomposedStringWithCanonicalMapping)}) { return true }
    }
    return children(element).contains { namesMatch($0,names,depth:depth+1) }
}
func selected(_ element: AXUIElement, names: [String], depth: Int = 0) -> AXUIElement? {
    if depth > 15 { return nil }
    if (attribute(element,kAXSelectedAttribute) as? Bool) == true,
       point(element,kAXPositionAttribute) != nil, namesMatch(element,names) { return element }
    for child in children(element) { if let found = selected(child,names:names,depth:depth+1) { return found } }
    return nil
}
func wait(_ condition: () -> Bool) -> Bool {
    for _ in 0..<80 { if condition() { return true }; RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1)) }
    return false
}
func mouse(_ type: CGEventType, _ at: CGPoint) {
    guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left) else { fail("Cannot create mouse event") }
    event.post(tap: .cghidEventTap)
}
func setFrame(_ window: AXUIElement, _ position: CGPoint, _ dimensions: CGSize) {
    var p = position; var s = dimensions
    if let value = AXValueCreate(.cgPoint, &p) { AXUIElementSetAttributeValue(window,kAXPositionAttribute as CFString,value) }
    if let value = AXValueCreate(.cgSize, &s) { AXUIElementSetAttributeValue(window,kAXSizeAttribute as CFString,value) }
}
func windows(_ pid: pid_t) -> [AXUIElement] { attribute(AXUIElementCreateApplication(pid),kAXWindowsAttribute) as? [AXUIElement] ?? [] }
let args = Array(CommandLine.arguments.dropFirst())
if args == ["--preflight"] {
    print("{\"accessibility\":\(AXIsProcessTrusted()),\"postEvents\":\(CGPreflightPostEventAccess())}")
    exit(AXIsProcessTrusted() && CGPreflightPostEventAccess() ? 0 : 2)
}
guard AXIsProcessTrusted(), CGPreflightPostEventAccess() else { fail("Native desktop tests need Accessibility access for this runner in System Settings > Privacy & Security > Accessibility.") }
if args.count == 3, args[0] == "--close-window", let pid = Int32(args[1]) {
    guard let window = windows(pid).first(where:{(attribute($0,kAXTitleAttribute) as? String)==args[2]}),
          let button = attribute(window,kAXCloseButtonAttribute) else { fail("No matching native test window close button") }
    guard AXUIElementPerformAction(unsafeBitCast(button,to:AXUIElement.self),kAXPressAction as CFString) == .success else { fail("Native window refused close") }
    guard wait({!windows(pid).contains(where:{(attribute($0,kAXTitleAttribute) as? String)==args[2]})}) else { fail("Native test window did not close") }
    print("{\"closed\":true}");exit(0)
}
if args.count == 2, args[0] == "--layout", let pid = Int32(args[1]), let window = windows(pid).first {
    var p = CGPoint(x:500,y:50)
    if let value = AXValueCreate(.cgPoint,&p) { AXUIElementSetAttributeValue(window,kAXPositionAttribute as CFString,value) }
    AXUIElementPerformAction(window,kAXRaiseAction as CFString)
    NSRunningApplication(processIdentifier:pid)?.activate(options:[.activateAllWindows])
    Thread.sleep(forTimeInterval:0.4)
    print("{\"layout\":true}"); exit(0)
}
guard args.count >= 5, let pid = Int32(args[0]), let x = Double(args[1]), let y = Double(args[2]), let viewport = Double(args[3]) else { fail("Usage: native-input <app-pid> <client-x> <client-y> <viewport-height> <files...> [--cancel]") }
let cancel = args.contains("--cancel")
let files = args.dropFirst(4).filter { $0 != "--cancel" }.map { URL(fileURLWithPath: $0) }
guard !files.isEmpty, files.allSatisfy({ FileManager.default.fileExists(atPath: $0.path) }), let folder = files.first?.deletingLastPathComponent(), files.allSatisfy({$0.deletingLastPathComponent() == folder}) else { fail("Drag source must be existing files in one fixture folder") }
guard let appWindow = windows(pid).first, let appSize = size(appWindow) else { fail("No test app window") }
// Keep the test destination clear of Finder. All coordinates are screen points.
// Window was laid out before Node measured its DOM target.
_ = appSize
AXUIElementPerformAction(appWindow,kAXRaiseAction as CFString)
let priorFinder = NSRunningApplication.runningApplications(withBundleIdentifier:"com.apple.finder").first
let priorWindows = priorFinder.map { windows($0.processIdentifier) } ?? []
NSWorkspace.shared.activateFileViewerSelecting(files)
guard let finder = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.finder").first else { fail("Finder is not running") }
var finderWindow: AXUIElement?
guard wait({ finderWindow = windows(finder.processIdentifier).first { (attribute($0,kAXTitleAttribute) as? String) == folder.lastPathComponent }; return finderWindow != nil }), let finderWindow else { fail("Finder did not reveal the fixture folder") }
let reused = priorWindows.contains { CFEqual($0,finderWindow) }
let oldPosition = point(finderWindow,kAXPositionAttribute)
let oldSize = size(finderWindow)
defer {
    if reused {
        if let oldPosition, let oldSize { setFrame(finderWindow,oldPosition,oldSize) }
    } else if let close = attribute(finderWindow,kAXCloseButtonAttribute) {
        AXUIElementPerformAction(unsafeBitCast(close,to:AXUIElement.self),kAXPressAction as CFString)
    }
}
setFrame(finderWindow, CGPoint(x:0,y:50),CGSize(width:480,height:650))
finder.activate(options: [.activateAllWindows])
var item: AXUIElement?
guard wait({ item = selected(finderWindow,names:files.flatMap { [$0.lastPathComponent,$0.deletingPathExtension().lastPathComponent] }); return item != nil }), let item, let origin = point(item,kAXPositionAttribute), let itemSize = size(item) else { fail("Finder has no selected fixture row or icon") }
// Node measured this DOM target after fixing the viewport and screen position.
guard let appOrigin = point(appWindow,kAXPositionAttribute), let finalSize = size(appWindow) else { fail("Cannot read test window bounds") }
let start = CGPoint(x:origin.x+min(itemSize.width/2,150),y:origin.y+itemSize.height/2)
let destination = CGPoint(x: appOrigin.x+x, y:appOrigin.y+finalSize.height-viewport+y)
mouse(.mouseMoved,start); Thread.sleep(forTimeInterval:0.15)
mouse(.leftMouseDown,start); Thread.sleep(forTimeInterval:0.2)
for step in 1...45 {
    let t = Double(step)/45
    mouse(.leftMouseDragged,CGPoint(x:start.x+(destination.x-start.x)*t,y:start.y+(destination.y-start.y)*t))
    Thread.sleep(forTimeInterval:0.025)
}
Thread.sleep(forTimeInterval:0.4)
if cancel {
    let down = CGEvent(keyboardEventSource:nil,virtualKey:53,keyDown:true)
    let up = CGEvent(keyboardEventSource:nil,virtualKey:53,keyDown:false)
    down?.post(tap:.cghidEventTap); up?.post(tap:.cghidEventTap)
    Thread.sleep(forTimeInterval:0.1)
}
mouse(.leftMouseUp,destination)
Thread.sleep(forTimeInterval:0.7)
print("{\"nativeDrag\":true,\"cancelled\":\(cancel),\"files\":\(files.count)}")
