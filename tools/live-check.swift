// Checks the running rift against what macOS lets it do: the facts the fork
// depends on that unit tests cannot reach (window ordering, overlay windows,
// screen capture, the event tap). See tools/live-check for how to run it.
//
//   live-check windows X,Y TITLE...  open plain windows with these titles at a
//                                    top-left point, and wait
//   live-check check [UUID]       run the checks on that display, or the active one

import AppKit
import CoreGraphics
import Foundation

// MARK: - Test windows

/// Open the windows at `at`, a top-left point in WindowServer coordinates, so
/// they appear on the display being checked: macOS otherwise opens them on the
/// display of the active app, whichever rift is testing.
func openWindows(at: CGPoint, _ titles: [String]) {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    var windows: [NSWindow] = []
    // AppKit counts up from the bottom of the main display.
    let top = NSScreen.screens.first?.frame.maxY ?? 0
    for (i, title) in titles.enumerated() {
        let origin = CGPoint(x: at.x + CGFloat(i * 60), y: top - at.y - CGFloat(i * 60) - 320)
        let w = NSWindow(
            contentRect: NSRect(origin: origin, size: CGSize(width: 480, height: 320)),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered, defer: false)
        w.title = title
        w.backgroundColor = .windowBackgroundColor
        w.isReleasedWhenClosed = false
        w.makeKeyAndOrderFront(nil)
        windows.append(w)
    }
    app.activate(ignoringOtherApps: true)
    app.run()
}

// MARK: - Helpers

struct Fail: Error { let message: String }

func riftCLI(_ args: [String]) throws -> Any {
    let p = Process()
    let candidates = [NSHomeDirectory() + "/.local/bin/rift-cli", "/opt/homebrew/bin/rift-cli", "/usr/local/bin/rift-cli"]
    guard let path = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
        throw Fail(message: "rift-cli not found")
    }
    p.executableURL = URL(fileURLWithPath: path)
    p.arguments = args
    let out = Pipe()
    p.standardOutput = out
    p.standardError = Pipe()
    try p.run()
    let data = out.fileHandleForReading.readDataToEndOfFile()
    p.waitUntilExit()
    guard p.terminationStatus == 0 else { throw Fail(message: "rift-cli \(args.joined(separator: " ")) failed") }
    return (try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])) ?? ""
}

func rect(_ any: Any?) -> CGRect {
    guard let d = any as? [String: Any], let o = d["origin"] as? [String: Double], let s = d["size"] as? [String: Double]
    else { return .null }
    return CGRect(x: o["x"]!, y: o["y"]!, width: s["width"]!, height: s["height"]!)
}

struct ServerWindow {
    let id: UInt32, pid: Int32, layer: Int, bounds: CGRect, owner: String
}

/// On-screen windows, front to back.
func serverWindows() -> [ServerWindow] {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.map { w in
        let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
        return ServerWindow(
            id: w[kCGWindowNumber as String] as? UInt32 ?? 0,
            pid: w[kCGWindowOwnerPID as String] as? Int32 ?? 0,
            layer: w[kCGWindowLayer as String] as? Int ?? 0,
            bounds: CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0),
            owner: w[kCGWindowOwnerName as String] as? String ?? "")
    }
}

func poll(_ timeout: Double, every: Double = 0.02, _ body: () -> Bool) -> Bool {
    let end = Date().addingTimeInterval(timeout)
    repeat {
        if body() { return true }
        Thread.sleep(forTimeInterval: every)
    } while Date() < end
    return false
}

func ms(since t: Date) -> Int { Int(Date().timeIntervalSince(t) * 1000) }

// MARK: - Checks

struct Result { let name: String; let ok: Bool; let detail: String }
var results: [Result] = []

func record(_ name: String, _ ok: Bool, _ detail: String) {
    results.append(Result(name: name, ok: ok, detail: detail))
    print("\(ok ? "ok  " : "FAIL") \(name)  \(detail)")
    fflush(stdout)
}

func check(display wanted: String?) throws {
    // rift and its settings
    let riftPids = serverPidsNamed("rift")
    record("rift.process", !riftPids.isEmpty, "pids \(riftPids)")
    guard let rift = riftPids.first else { return }
    let config = (try riftCLI(["execute", "config", "get"]) as? [String: Any])?["settings"] as? [String: Any] ?? [:]
    let ui = config["ui"] as? [String: Any] ?? [:]
    let enabled = { (key: String) -> Bool in (ui[key] as? [String: Any])?["enabled"] as? Bool ?? false }
    let motion = ui["grid_motion"] as? [String: Any] ?? [:]
    let pan = motion["pan"] as? Bool ?? true, hud = motion["hud"] as? Bool ?? true
    let displays = try riftCLI(["query", "displays"]) as? [[String: Any]] ?? []
    record("rift.displays", !displays.isEmpty, "\(displays.count) displays")

    // The event tap that hotkeys and the overview's input come through.
    var count: UInt32 = 0
    CGGetEventTapList(0, nil, &count)
    var taps = [CGEventTapInformation](repeating: CGEventTapInformation(), count: Int(count))
    CGGetEventTapList(count, &taps, &count)
    let mine = taps.filter { $0.tappingProcess == rift }
    record("input.event-tap", mine.contains { $0.enabled }, "\(mine.count) taps, \(mine.filter { $0.enabled }.count) enabled")

    guard let display = displays.first(where: { ($0["uuid"] as? String) == wanted })
            ?? displays.first(where: { $0["is_active_context"] as? Bool ?? false }) ?? displays.first,
          let uuid = display["uuid"] as? String
    else { return }
    let visible = rect(display["frame"])
    print("     on \(display["name"] as? String ?? uuid) \(visible)")

    // An empty cell with an empty neighbor in the grid on screen, to test in
    // without touching anyone's windows.
    let workspaces = { try riftCLI(["query", "workspaces", "--display", uuid]) as? [[String: Any]] ?? [] }
    var list = try workspaces()
    guard let original = list.first(where: { $0["is_active"] as? Bool ?? false })?["index"] as? Int else {
        record("grid.cells", false, "no active workspace on that display (fullscreen space?)")
        return
    }
    let columns = 3, perGrid = 9
    let grid = original / perGrid
    let empty = { (i: Int) in (list.first { $0["index"] as? Int == i }?["window_count"] as? Int ?? 1) == 0 }
    let directions: [(String, Int, Int)] = [("right", 1, 0), ("left", -1, 0), ("down", 0, 1), ("up", 0, -1)]
    var choice: (Int, Int, String)?
    search: for cell in 0..<perGrid {
        let index = grid * perGrid + cell
        guard empty(index) else { continue }
        for (name, dx, dy) in directions {
            let (r, c) = (cell / columns + dy, cell % columns + dx)
            guard (0..<columns).contains(r), (0..<columns).contains(c) else { continue }
            let neighbor = grid * perGrid + r * columns + c
            if empty(neighbor) { choice = (index, neighbor, name); break search }
        }
    }
    guard let (home, neighbor, direction) = choice else {
        record("grid.cells", false, "no two empty cells side by side in grid \(grid + 1)")
        return
    }
    let opposite = ["right": "left", "left": "right", "down": "up", "up": "down"][direction]!
    let active = { (try? workspaces())?.first(where: { $0["is_active"] as? Bool ?? false })?["index"] as? Int }
    defer { _ = try? riftCLI(["execute", "workspace", "switch", "\(original)"]) }
    _ = try riftCLI(["execute", "display", "focus", "--uuid", uuid])
    _ = try riftCLI(["execute", "workspace", "switch", "\(home)"])
    record("grid.switch", poll(1.5) { active() == home }, "to cell \(home % perGrid + 1) of grid \(grid + 1)")

    // Two plain windows, tiled into that cell.
    let helper = Process()
    helper.executableURL = URL(fileURLWithPath: CommandLine.arguments[0])
    helper.arguments = ["windows", "\(Int(visible.minX + 80)),\(Int(visible.minY + 80))", "rift-check A", "rift-check B"]
    try helper.run()
    defer { helper.terminate() }
    var tiles: [String: CGRect] = [:]
    var serverIds: [String: UInt32] = [:]
    var riftIds: [String: String] = [:]
    let tiled = poll(6, every: 0.1) {
        list = (try? workspaces()) ?? []
        guard let ws = list.first(where: { $0["index"] as? Int == home }),
              let windows = ws["windows"] as? [[String: Any]] else { return false }
        for w in windows {
            guard let title = w["title"] as? String, title.hasPrefix("rift-check") else { continue }
            tiles[title] = rect(w["frame"])
            serverIds[title] = w["window_server_id"] as? UInt32
            if let id = w["id"], let json = try? JSONSerialization.data(withJSONObject: id) {
                riftIds[title] = String(data: json, encoding: .utf8)
            }
        }
        return tiles.count == 2
    }
    let a = tiles["rift-check A"] ?? .null, b = tiles["rift-check B"] ?? .null
    record("layout.managed", tiled, "both test windows in the cell")
    guard tiled else { return }
    Thread.sleep(forTimeInterval: 0.5)
    let onServer = { (id: UInt32?) in serverWindows().first { $0.id == id }?.bounds ?? .null }
    let placed = [a, b].allSatisfy { visible.insetBy(dx: -2, dy: -2).contains($0) } && !a.intersects(b)
    record("layout.tiles", placed && onServer(serverIds["rift-check A"]).equalTo(a),
           "A \(a.integral) B \(b.integral), A on screen \(onServer(serverIds["rift-check A"]).integral)")

    // Borders: bands around the focused window, ordered just above it.
    if enabled("borders") {
        _ = try riftCLI(["execute", "window", "focus", "--window-id", riftIds["rift-check A"] ?? "",
                         "--window-server-id", "\(serverIds["rift-check A"]!)"])
        var bands: [ServerWindow] = []
        var order = false
        let drawn = poll(1.5, every: 0.05) {
            let all = serverWindows()
            let around = a.insetBy(dx: -48, dy: -48)
            bands = all.filter { $0.pid == rift && $0.layer == 0 && around.contains($0.bounds) && !$0.bounds.contains(a) }
            guard let target = all.firstIndex(where: { $0.id == serverIds["rift-check A"] }) else { return false }
            order = !bands.isEmpty && bands.allSatisfy { band in all.firstIndex { $0.id == band.id }! < target }
            return bands.count >= 4
        }
        record("borders.bands", drawn, "\(bands.count) border windows around the focused window")
        record("borders.order", order, "every band in front of its window")
    }

    // A move to the next cell: the windows leave the screen, the slide and the
    // map show, and coming back puts the windows where they were.
    var panAt: Int?, panGone: Int?, hudAt: Int?, hudGone: Int?
    let t0 = Date()
    _ = try riftCLI(["execute", "layout", "grid-cell", direction])
    _ = poll(2.5, every: 0.015) {
        let mine = serverWindows().filter { $0.pid == rift }
        let p = mine.contains { $0.layer >= 100 && $0.bounds.equalTo(visible) }
        let h = mine.contains { $0.layer >= 100 && $0.bounds.width < 400 }
        if p && panAt == nil { panAt = ms(since: t0) }
        if !p && panAt != nil && panGone == nil { panGone = ms(since: t0) }
        if h && hudAt == nil { hudAt = ms(since: t0) }
        if !h && hudAt != nil && hudGone == nil { hudGone = ms(since: t0) }
        return panGone != nil && hudGone != nil
    }
    let moved = poll(1, every: 0.05) { active() == neighbor }
    let hidden = poll(1, every: 0.05) {
        [serverIds["rift-check A"], serverIds["rift-check B"]].allSatisfy { id in
            // rift leaves a hidden window a sliver on screen, a point or so wide.
            let r = onServer(id).intersection(visible)
            return r.isNull || min(r.width, r.height) <= 2
        }
    }
    let left = [serverIds["rift-check A"], serverIds["rift-check B"]].map { onServer($0).integral }
    record("grid.cell-move", moved && hidden,
           "\(direction): active cell \(moved ? "changed" : "unchanged"), test windows at \(left)")
    if pan {
        record("motion.pan", panAt != nil && panGone != nil,
               "overlay after \(panAt.map { "\($0) ms" } ?? "-"), gone after \(panGone.map { "\($0) ms" } ?? "-")")
    }
    if hud {
        record("motion.hud", hudAt != nil && hudGone != nil,
               "map after \(hudAt.map { "\($0) ms" } ?? "-"), gone after \(hudGone.map { "\($0) ms" } ?? "-")")
    }
    Thread.sleep(forTimeInterval: 0.3)
    _ = try riftCLI(["execute", "layout", "grid-cell", opposite])
    let back = poll(1.5, every: 0.05) {
        active() == home && [("rift-check A", a), ("rift-check B", b)].allSatisfy { title, frame in
            let r = onServer(serverIds[title])
            return abs(r.minX - frame.minX) <= 2 && abs(r.minY - frame.minY) <= 2
                && abs(r.width - frame.width) <= 2 && abs(r.height - frame.height) <= 2
        }
    }
    record("grid.cell-return", back, "\(opposite): test windows back where they were")
    Thread.sleep(forTimeInterval: 1.2)

    // The overview: its own windows over each display, gone when dismissed.
    if enabled("mission_control") {
        _ = try riftCLI(["execute", "mission-control", "show-all"])
        var covered = 0
        let open = poll(1.5, every: 0.03) {
            covered = serverWindows().filter { $0.pid == rift && $0.layer >= 100 && $0.bounds.width > 400 }.count
            return covered >= 1
        }
        record("overview.open", open, "\(covered) overview windows")
        Thread.sleep(forTimeInterval: 0.6)
        _ = try riftCLI(["execute", "mission-control", "dismiss"])
        let closed = poll(1.5, every: 0.03) {
            !serverWindows().contains { $0.pid == rift && $0.layer >= 100 && $0.bounds.width > 400 }
        }
        record("overview.close", closed, "overview windows gone")
    }
}

/// Processes running `name` with no arguments: rift itself, not `rift --settings`.
func serverPidsNamed(_ name: String) -> [Int32] {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/ps")
    p.arguments = ["-axo", "pid=,args="]
    let out = Pipe()
    p.standardOutput = out
    try? p.run()
    let text = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    p.waitUntilExit()
    return text.split(separator: "\n").compactMap { line in
        let parts = line.trimmingCharacters(in: .whitespaces).split(separator: " ", maxSplits: 1)
        guard parts.count == 2, parts[1] == name || parts[1].hasSuffix("/" + name) else { return nil }
        return Int32(parts[0])
    }
}

// MARK: - Main

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "windows":
    let xy = args.count > 1 ? args[1].split(separator: ",").compactMap { Double($0) } : []
    guard xy.count == 2 else { print("usage: live-check windows X,Y TITLE..."); exit(2) }
    openWindows(at: CGPoint(x: xy[0], y: xy[1]), Array(args.dropFirst(2)))
case "check":
    do {
        try check(display: args.count > 1 ? args[1] : nil)
    } catch let error as Fail {
        record("run", false, error.message)
    }
    let failed = results.filter { !$0.ok }.count
    print("\(results.count - failed) ok, \(failed) failed")
    if let out = ProcessInfo.processInfo.environment["LIVE_CHECK_OUT"] {
        let lines = results.map { "\($0.ok ? "ok" : "FAIL") \($0.name)" }.joined(separator: "\n") + "\n"
        try? lines.write(toFile: out, atomically: true, encoding: .utf8)
    }
    exit(failed == 0 ? 0 : 1)
default:
    print("usage: live-check check [display-uuid] | live-check windows X,Y TITLE...")
    exit(2)
}
