// Bring the application with the given PID to the front so its window is
// unoccluded — a hidden window has frames disabled, and a hot reload applied
// while hidden is rebuilt only once the window is visible again.
//
// Usage: macos-activate PID
import AppKit

let args = CommandLine.arguments
guard args.count == 2, let pid = Int32(args[1]) else {
    FileHandle.standardError.write("usage: macos-activate PID\n".data(using: .utf8)!)
    exit(2)
}
guard let app = NSRunningApplication(processIdentifier: pid) else {
    print("NO_SUCH_PROCESS pid=\(pid)")
    exit(1)
}
let activated = app.activate(options: [.activateAllWindows])
print("activated=\(activated) pid=\(pid) active=\(app.isActive)")
