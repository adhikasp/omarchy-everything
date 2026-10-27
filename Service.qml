import QtQuick
import Quickshell
import Quickshell.Io
import "EverythingModel.js" as Model

// Shell-wide state and helper lifecycle. Bar instances acquire a lease only
// while their results mode is open. The Python helper therefore exists only
// while at least one panel needs discovery, or an activation is still in
// flight; the shell-only group checklist does not start it.
Item {
  id: root

  property var manifest: null
  property var shell: null
  property string omarchyPath: ""

  property var items: []
  property var providerItems: ({})
  property var activationTokens: ({})
  property var warnings: []
  property var hiddenIds: ({})
  property var leases: ({})
  property int leaseCount: 0
  property int pendingActivations: 0
  property int requestSerial: 0
  property string activeScanId: ""
  property bool helperReady: false
  property bool scanning: false
  property bool intentionalStop: false
  property int crashCount: 0
  property var requestQueue: []
  property var helperRows: []
  property var localRows: []

  // Quattro sanitizes third-party manifests. Resolve co-located code from
  // this component, decoding the file URL before passing it as an argv path.
  readonly property string helperPath: decodeURIComponent(
    String(Qt.resolvedUrl("helper/everything_helper.py")).replace(/^file:\/\//, ""))
  readonly property bool ghosttyPaletteEnabled: Object.keys(leases).some(function(key) {
    return leases[key] === true
  })

  function nextRequestId(prefix) {
    requestSerial += 1
    return String(prefix || "request") + "-" + requestSerial
  }

  function acquire(ownerKey, ghosttyPalette) {
    var key = String(ownerKey || "panel")
    var enabled = ghosttyPalette === true
    if (leases[key] === enabled) return
    var next = {}
    for (var existing in leases) next[existing] = leases[existing]
    next[key] = enabled
    leases = next
    leaseCount = Object.keys(next).length
    refreshLocalRows()
    ensureHelper()
    requestScan(enabled)
  }

  function release(ownerKey) {
    var key = String(ownerKey || "panel")
    if (leases[key] === undefined) return
    var hadPalette = ghosttyPaletteEnabled
    var next = {}
    for (var existing in leases) if (existing !== key) next[existing] = leases[existing]
    leases = next
    leaseCount = Object.keys(next).length
    if (hadPalette && !ghosttyPaletteEnabled && leaseCount > 0) requestScan(false)
    maybeStopTimer.restart()
  }

  function hideItem(id) {
    hiddenIds = Model.withHidden(hiddenIds, id)
  }

  function requestScan(includeGhostty) {
    if (leaseCount <= 0) return
    var requestId = nextRequestId("scan")
    activeScanId = requestId
    scanning = true
    sendRequest({
      version: 1,
      id: requestId,
      type: "scan",
      options: {
        ghostty: includeGhostty === true && ghosttyPaletteEnabled,
        ghosttyPalette: ghosttyPaletteEnabled
      }
    })
  }

  function activate(item) {
    if (!item || !item.id) return
    if (item.local) { activateLocal(item); return }
    var token = String(activationTokens[String(item.id)] || item.activationToken || "")
    if (!token) return
    pendingActivations += 1
    sendRequest({
      version: 1,
      id: nextRequestId("activate"),
      type: "activate",
      itemId: String(item.id),
      token: token
    })
  }

  function sendRequest(request) {
    ensureHelper()
    if (!helperReady || !helper.running) {
      var next = requestQueue.slice()
      next.push(request)
      requestQueue = next
      return
    }
    helper.write(JSON.stringify(request) + "\n")
  }

  function flushQueue() {
    if (!helperReady || !helper.running) return
    var queued = requestQueue
    requestQueue = []
    for (var i = 0; i < queued.length; i++)
      helper.write(JSON.stringify(queued[i]) + "\n")
  }

  function ensureHelper() {
    if (helper.running || !helperPath) return
    intentionalStop = false
    // Prefer the Rust helper (~1 ms cold start); fall back to the Python one,
    // which also covers AT-SPI, Ghostty, Kitty, tmux and Neovim.
    helper.command = ["sh", "-c",
      'rust="${1%/helper/everything_helper.py}/rust/target/release/everything-helper"; ' +
      '[ -x "$rust" ] && exec "$rust" --json-lines; exec python3 "$1" --json-lines',
      "everything-helper", helperPath]
    helper.running = true
  }

  function stopHelper() {
    if (!helper.running) return
    intentionalStop = true
    if (helperReady) {
      helper.write(JSON.stringify({
        version: 1,
        id: nextRequestId("shutdown"),
        type: "shutdown"
      }) + "\n")
      forceStopTimer.restart()
    } else {
      helper.running = false
    }
  }

  function maybeStop() {
    if (leaseCount === 0 && pendingActivations === 0) stopHelper()
  }

  function mergePartial(provider, rows) {
    if (!provider) return
    var next = {}
    for (var key in providerItems) next[key] = providerItems[key]
    next[String(provider)] = Array.isArray(rows) ? rows : []
    providerItems = next
    publishItems(Model.mergeProviderItems(next))
  }

  // Launcher rows come from the shell's AppLibrary and a fixed Omarchy menu
  // route list, not the helper. Rebuilt on every open so new installs appear.
  readonly property var menuRoutes: [
    { id: "apps", title: "Apps", terms: ["launcher", "applications"], argv: ["omarchy-menu", "summon", "apps"] },
    { id: "learn", title: "Learn", terms: ["help", "docs", "manual"], argv: ["omarchy-menu", "summon", "learn"] },
    { id: "trigger", title: "Trigger", terms: ["emoji", "capture", "screenshot", "toggle"], argv: ["omarchy-menu", "summon", "trigger"] },
    { id: "style", title: "Style", terms: ["appearance", "look", "font"], argv: ["omarchy-menu", "summon", "style"] },
    { id: "style.theme", title: "Style: Theme", terms: ["colors", "colour"], argv: ["omarchy-menu", "summon", "theme"] },
    { id: "style.background", title: "Style: Background", terms: ["wallpaper"], argv: ["omarchy-menu", "summon", "background"] },
    { id: "setup", title: "Setup (Settings)", terms: ["settings", "config", "preferences"], argv: ["omarchy-menu", "summon", "setup"] },
    { id: "install", title: "Install", terms: ["add", "package", "pacman"], argv: ["omarchy-menu", "summon", "install"] },
    { id: "remove", title: "Remove (Uninstall)", terms: ["uninstall", "delete"], argv: ["omarchy-menu", "summon", "remove"] },
    { id: "update", title: "Update", terms: ["upgrade", "restart", "refresh"], argv: ["omarchy-menu", "summon", "update"] },
    { id: "system", title: "System (Power)", terms: ["power", "shutdown", "reboot", "logout"], argv: ["omarchy-menu", "summon", "system"] },
    { id: "system.lock", title: "System: Lock", terms: ["lock screen"], argv: ["omarchy-system-lock"] },
    { id: "system.suspend", title: "System: Suspend", argv: ["systemctl", "suspend"] },
    { id: "system.logout", title: "System: Logout", terms: ["sign out", "log out"], argv: ["omarchy-system-logout"] },
    { id: "system.reboot", title: "System: Reboot", terms: ["restart"], argv: ["omarchy-system-reboot"] },
    { id: "system.shutdown", title: "System: Shutdown", terms: ["power off", "turn off"], argv: ["omarchy-system-shutdown"] },
    { id: "trigger.emoji", title: "Trigger: Emoji", terms: ["emojis", "smiley"], argv: ["omarchy-menu-emoji"] },
    { id: "learn.keybindings", title: "Learn: Keybindings", terms: ["shortcuts", "hotkeys", "binds"], argv: ["omarchy-menu-keybindings"] }
  ]

  function refreshLocalRows() {
    var rows = []
    var entries = DesktopEntries.applications.values || []
    for (var i = 0; i < entries.length; i++) {
      var entry = entries[i]
      if (!entry || entry.noDisplay) continue
      var appId = String(entry.id || "")
      var name = String(entry.name || "")
      if (!appId || !name) continue
      rows.push({
        id: "app:" + appId,
        kind: "app",
        provider: "Apps",
        title: name,
        iconHints: entry.icon ? [String(entry.icon)] : [],
        searchTerms: [String(entry.genericName || "")].concat(
          Array.isArray(entry.keywords) ? entry.keywords.map(String) : []).filter(Boolean),
        local: { type: "app", appId: appId }
      })
    }
    for (var m = 0; m < menuRoutes.length; m++) {
      var route = menuRoutes[m]
      rows.push({
        id: "omarchy-menu:" + route.id,
        kind: "omarchy-menu",
        provider: "Omarchy",
        title: route.title,
        iconHints: [],
        searchTerms: route.terms || [],
        local: { type: "exec", argv: route.argv }
      })
    }
    localRows = rows
    publishItems(helperRows)
  }

  function activateLocal(item) {
    var action = item.local
    if (action.type === "app") {
      var entries = DesktopEntries.applications.values || []
      for (var i = 0; i < entries.length; i++) {
        if (String(entries[i].id || "") === action.appId) { entries[i].execute(); break }
      }
    } else if (action.type === "exec" && Array.isArray(action.argv)) {
      Quickshell.execDetached(action.argv)
    }
  }

  function publishItems(rows) {
    helperRows = Array.isArray(rows) ? rows : []
    var incoming = helperRows.concat(localRows)
    var tokens = {}
    for (var i = 0; i < incoming.length; i++) {
      var row = incoming[i]
      if (row && row.id && row.activationToken)
        tokens[String(row.id)] = String(row.activationToken)
    }
    activationTokens = tokens
    var reconciled = Model.reconcileItems(items, incoming)
    if (reconciled.changed) items = reconciled.items
  }

  function notifyFailure(message) {
    var text = String(message || "The thing is no longer available")
    if (!omarchyPath) return
    Quickshell.execDetached([
      omarchyPath + "/bin/omarchy-notification-send",
      "--app-name", "Everything",
      "Everything",
      text
    ])
  }

  function appendWarning(message) {
    var value = String(message || "").trim()
    if (!value) return
    var next = warnings.filter(function(entry) { return entry !== value })
    next.push(value)
    warnings = next.slice(Math.max(0, next.length - 8))
  }

  function handleMessage(line) {
    var message
    try {
      message = JSON.parse(String(line || ""))
    } catch (error) {
      appendWarning("Helper returned malformed data")
      return
    }
    if (!message || message.version !== 1 || !message.type) return

    if (message.type === "ready") {
      helperReady = true
      flushQueue()
      return
    }

    if (message.type === "snapshot") {
      var correlated = String(message.requestId || "") === activeScanId
      if (!correlated) return
      if (message.full === true) {
        publishItems(message.items)
        providerItems = message.providers && typeof message.providers === "object"
          ? message.providers : providerItems
        warnings = Array.isArray(message.warnings) ? message.warnings : warnings
        if (String(message.requestId || "") === activeScanId) {
          activeScanId = ""
          scanning = false
          crashCount = 0
        }
      } else {
        mergePartial(message.provider, message.items)
        if (Array.isArray(message.warnings))
          for (var i = 0; i < message.warnings.length; i++) appendWarning(message.warnings[i])
      }
      return
    }

    if (message.type === "activation") {
      pendingActivations = Math.max(0, pendingActivations - 1)
      if (message.ok !== true) notifyFailure(message.message || (message.stale === true
        ? "That thing closed before it could be focused"
        : "Could not focus that thing"))
      maybeStopTimer.restart()
      return
    }

    if (message.type === "error") {
      appendWarning(message.message || "A thing provider failed")
      if (message.requestId && String(message.requestId) === activeScanId && message.final === true)
        scanning = false
    }
  }

  Process {
    id: helper
    stdinEnabled: true

    stdout: SplitParser {
      onRead: function(line) { root.handleMessage(line) }
    }

    stderr: SplitParser {
      onRead: function(line) {
        var value = String(line || "").trim()
        if (value) root.appendWarning(value)
      }
    }

    onExited: function(exitCode, _exitStatus) {
      forceStopTimer.stop()
      var hadPendingActivation = root.pendingActivations > 0
      root.helperReady = false
      root.scanning = false
      root.activeScanId = ""
      if (root.intentionalStop) {
        root.intentionalStop = false
        root.requestQueue = []
        root.pendingActivations = 0
        if (hadPendingActivation)
          root.notifyFailure("Everything stopped before it could focus the thing")
        if (root.leaseCount > 0) restartTimer.restart()
        return
      }
      root.requestQueue = []
      root.pendingActivations = 0
      if (hadPendingActivation)
        root.notifyFailure("Everything stopped before it could focus the thing")
      if (root.leaseCount > 0 || root.pendingActivations > 0) {
        root.crashCount += 1
        root.appendWarning("Everything helper stopped unexpectedly (exit " + exitCode + ")")
        if (root.crashCount <= 3) restartTimer.restart()
      }
    }
  }

  Timer {
    id: pollTimer
    interval: 2000
    repeat: true
    running: root.leaseCount > 0 && root.helperReady
    onTriggered: if (!root.scanning) root.requestScan(false)
  }

  // Keep the helper warm after the panel closes: a cold start costs ~300 ms of
  // Python and AT-SPI guard startup. It does not poll while idle (no leases).
  Timer {
    id: maybeStopTimer
    interval: 120000
    repeat: false
    onTriggered: root.maybeStop()
  }

  Timer {
    id: forceStopTimer
    interval: 1000
    repeat: false
    onTriggered: if (helper.running) helper.running = false
  }

  Timer {
    id: restartTimer
    interval: 750
    repeat: false
    onTriggered: {
      root.ensureHelper()
      if (root.leaseCount > 0) root.requestScan(true)
    }
  }

  Component.onDestruction: {
    if (helper.running) {
      intentionalStop = true
      if (helperReady) helper.write(JSON.stringify({ version: 1, id: "destruction", type: "shutdown" }) + "\n")
      helper.running = false
    }
  }
}
