import QtQuick
import Quickshell
import Quickshell.Io
import "EverythingModel.js" as Model
import "InstantAnswers.js" as Answers

// Shell-wide state and helper lifecycle. Bar instances acquire a lease only
// while their results mode is open. The helper process therefore exists only
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
    String(Qt.resolvedUrl("rust/target/release/everything-helper")).replace(/^file:\/\//, ""))

  function nextRequestId(prefix) {
    requestSerial += 1
    return String(prefix || "request") + "-" + requestSerial
  }

  function acquire(ownerKey) {
    var key = String(ownerKey || "panel")
    if (leases[key] === true) return
    var next = {}
    for (var existing in leases) next[existing] = leases[existing]
    next[key] = true
    leases = next
    leaseCount = Object.keys(next).length
    refreshLocalRows()
    ensureHelper()
    requestScan()
  }

  function release(ownerKey) {
    var key = String(ownerKey || "panel")
    if (leases[key] === undefined) return
    var next = {}
    for (var existing in leases) if (existing !== key) next[existing] = leases[existing]
    leases = next
    leaseCount = Object.keys(next).length
    maybeStopTimer.restart()
  }

  function hideItem(id) {
    hiddenIds = Model.withHidden(hiddenIds, id)
  }

  function requestScan() {
    if (leaseCount <= 0) return
    var requestId = nextRequestId("scan")
    activeScanId = requestId
    scanning = true
    sendRequest({
      version: 1,
      id: requestId,
      type: "scan",
      options: {}
    })
  }

  function activate(item) {
    if (!item || !item.id) return
    if (item.local) {
      activateLocal(item)
      // A timer was started or cleared: the cached list is now wrong.
      if (item.kind === "timer") {
        remindersFetchedAt = 0
        reminderRefreshTimer.restart()
      }
      return
    }
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
    helper.command = [helperPath, "--json-lines"]
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

  // Every actionable leaf of the Omarchy menu (default + user extension),
  // flattened by a script so new menu entries appear here without edits.
  property var dynamicRoutes: []

  Process {
    id: menuRoutesProc
    command: [Quickshell.env("HOME") + "/.config/omarchy/plugins/io.github.adhikasp.everything/scripts/omarchy-menu-routes"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          root.dynamicRoutes = JSON.parse(text)
          root.refreshLocalRows(true)
        } catch (e) { console.warn("omarchy-menu-routes:", e) }
      }
    }
  }

  function refreshLocalRows(skipReload) {
    if (skipReload !== true && !menuRoutesProc.running) menuRoutesProc.running = true
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
    var seen = {}
    for (var h = 0; h < menuRoutes.length; h++) seen[menuRoutes[h].id] = true
    for (var d = 0; d < dynamicRoutes.length; d++) {
      var dr = dynamicRoutes[d]
      if (!dr || seen[dr.id]) continue
      rows.push({
        id: "omarchy-menu:" + dr.id,
        kind: "omarchy-menu",
        provider: "Omarchy",
        title: dr.title,
        iconHints: [],
        searchTerms: dr.terms || [],
        local: { type: "exec", argv: ["bash", "-lc", dr.action] }
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
    } else if (action.type === "copy") {
      // Instant answers: argv only, never a shell, so the value is inert.
      copyText(action.text, action.label || action.text)
    } else if (action.type === "paste-clip") {
      var index = Number(action.index)
      if (!isFinite(index) || index < 0 || Math.floor(index) !== index) return
      Quickshell.execDetached([(omarchyPath || "/usr/share/omarchy") + "/bin/omarchy-clipboard-paste-text",
        "--shift-insert", "--history-index", String(index)])
    } else if (action.type === "paste-image") {
      var imagePath = String(action.path || "")
      var mime = String(action.mime || "")
      if (imagePath.indexOf(homeDir + "/.local/state/omarchy/clipboard-images/") !== 0
          || imagePath.indexOf("..") >= 0 || !/^image\/(png|jpeg|gif|webp)$/.test(mime)) return
      Quickshell.execDetached(["sh", "-c",
        'wl-copy --type "$1" < "$2" && sleep 0.15 && wtype -M shift -k Insert -m shift',
        "sh", mime, imagePath])
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
        root.appendWarning("Everything helper stopped unexpectedly (exit " + exitCode + "); is it built? cd rust && cargo build --release")
        if (root.crashCount <= 3) restartTimer.restart()
      }
    }
  }

  Timer {
    id: pollTimer
    interval: 2000
    repeat: true
    running: root.leaseCount > 0 && root.helperReady
    onTriggered: if (!root.scanning) root.requestScan()
  }

  // Keep the helper warm after the panel closes: a cold start still costs a
  // scan. It does not poll while idle (no leases).
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
      if (root.leaseCount > 0) root.requestScan()
    }
  }

  // --- instant-answer data ------------------------------------------------
  //
  // InstantAnswers.js is pure; everything it needs from the outside world is
  // fetched here, lazily, only when a query asks for it (`needs`). Bumping
  // answerRevision makes open panels re-rank.

  property int answerRevision: 0
  readonly property string homeDir: Quickshell.env("HOME")

  function bumpAnswers() { answerRevision += 1 }

  function ensureAnswerData(needs) {
    if (!needs) return
    if (needs.rates) ensureRates()
    if (needs.tz) ensureZones()
    if (needs.clipboard) ensureClipboard()
    if (needs.reminders) refreshReminders()
  }

  // Currency: ECB reference rates from a fixed HTTPS endpoint, at most twice
  // a day, cached in ~/.cache. The request carries no user data.
  property var rates: null
  property string ratesError: ""
  property bool ratesLoading: false
  property real ratesAttemptAt: 0
  readonly property string ratesUrl: "https://api.frankfurter.dev/v1/latest?base=EUR"
  readonly property real ratesMaxAgeMs: 12 * 3600 * 1000

  function ensureRates() {
    var now = Date.now()
    if (rates && now - Number(rates.fetchedAt || 0) < ratesMaxAgeMs) return
    if (ratesLoading || now - ratesAttemptAt < 60000) return
    ratesLoading = true
    ratesAttemptAt = now
    var xhr = new XMLHttpRequest()
    ratesTimeout.xhr = xhr
    ratesTimeout.restart()
    xhr.onreadystatechange = function() {
      if (xhr.readyState !== XMLHttpRequest.DONE) return
      ratesTimeout.stop()
      root.ratesLoading = false
      var text = String(xhr.responseText || "")
      var parsed = null
      if (xhr.status === 200 && text.length < 20000) {
        try { parsed = Answers.validateRates(JSON.parse(text)) } catch (_error) { parsed = null }
      }
      if (parsed) {
        parsed.fetchedAt = Date.now()
        root.rates = parsed
        root.ratesError = ""
        ratesCache.setText(JSON.stringify(parsed) + "\n")
      } else if (!root.rates) {
        root.ratesError = xhr.status ? "HTTP " + xhr.status : "offline"
      }
      root.bumpAnswers()
    }
    xhr.open("GET", ratesUrl)
    xhr.send()
  }

  Timer {
    id: ratesTimeout
    property var xhr: null
    interval: 8000
    onTriggered: if (xhr) xhr.abort()
  }

  FileView {
    id: ratesCache
    path: root.homeDir + "/.cache/omarchy-everything-rates.json"
    printErrors: false
    onLoaded: {
      try {
        var cached = Answers.validateRates(JSON.parse(text()))
        if (cached && !root.rates) { root.rates = cached; root.bumpAnswers() }
      } catch (_error) { }
    }
  }

  // Time zones: today's UTC offset for each known zone, from tzdata via
  // `date`. The zone names are the plugin's own constants, passed as argv.
  property var tzOffsets: null
  property string localZone: "UTC"
  property real tzFetchedAt: 0

  function ensureZones() {
    if (tzProcess.running) return
    if (tzOffsets && Date.now() - tzFetchedAt < 30 * 60 * 1000) return
    tzProcess.command = ["sh", "-c",
      'for z do printf "%s %s\\n" "$z" "$(TZ="$z" date +%z)"; done; '
      + 'printf "LOCAL %s\\n" "$(timedatectl show -p Timezone --value 2>/dev/null)"',
      "sh"].concat(Answers.zoneList())
    tzProcess.running = true
  }

  Process {
    id: tzProcess
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var parsed = Answers.parseZoneOffsets(text)
        root.tzOffsets = parsed.offsets
        root.localZone = parsed.localZone
        root.tzFetchedAt = Date.now()
        root.bumpAnswers()
      }
    }
  }

  // Clipboard history: Omarchy's own store, read only while asked for.
  property var clipboardHistory: null
  property bool clipboardWanted: false

  function ensureClipboard() {
    if (clipboardWanted) return
    clipboardWanted = true
  }

  FileView {
    id: clipboardFile
    path: root.clipboardWanted ? root.homeDir + "/.local/state/omarchy/clipboard-history.json" : ""
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      root.clipboardHistory = Answers.parseClipboardHistory(text())
      root.bumpAnswers()
    }
    onLoadFailed: {
      root.clipboardHistory = []
      root.bumpAnswers()
    }
  }

  // Reminders: `omarchy reminder show --json`, refreshed per timer query.
  property var reminders: []
  property real remindersFetchedAt: 0

  function refreshReminders() {
    if (reminderProcess.running || Date.now() - remindersFetchedAt < 2000) return
    reminderProcess.running = true
  }

  Timer {
    id: reminderRefreshTimer
    interval: 700
    onTriggered: root.refreshReminders()
  }

  Process {
    id: reminderProcess
    command: ["omarchy", "reminder", "show", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.reminders = Answers.parseReminders(text)
        root.remindersFetchedAt = Date.now()
        root.bumpAnswers()
      }
    }
  }

  // Ask: one `opencode run` against the warm background service, in the
  // configured workspace. The prompt is a single argv entry, never shell text.
  property var askState: Answers.emptyAskState()
  property var lastAsk: null

  function expandHome(path) {
    var value = String(path || "~")
    if (value === "~") return homeDir
    if (value.indexOf("~/") === 0) return homeDir + value.slice(1)
    return value
  }

  function ask(prompt, workspace, model) {
    var text = String(prompt || "").trim()
    if (!text) return
    if (askProcess.running) askProcess.running = false
    var dir = expandHome(workspace)
    var next = Answers.emptyAskState()
    next.status = "running"
    next.prompt = text
    next.workspace = dir
    next.startedAt = Date.now()
    askState = next
    var argv = ["opencode", "run", "--format", "json", "--title", "Everything: " + text.slice(0, 60)]
    if (model && /^[A-Za-z0-9._\/:#-]+$/.test(String(model))) argv.push("--model", String(model))
    // Not "--": opencode 2.2's parser then sends the message twice. A prompt
    // that starts with "-" gets a leading space so it can never be a flag.
    argv.push(/^-/.test(text) ? " " + text : text)
    askProcess.workingDirectory = dir
    // `opencode run` reads stdin whenever it is not a TTY, and Process hands
    // it an open pipe, so it would wait for EOF forever. Run it with stdin
    // from /dev/null; the argv still passes through untouched as "$@".
    askProcess.command = ["sh", "-c", 'exec "$@" < /dev/null', "sh"].concat(argv)
    askProcess.running = true
    askTimeout.restart()
    bumpAnswers()
  }

  function cancelAsk() {
    if (!askProcess.running) return
    askTimeout.stop()
    askProcess.running = false
    askState = Answers.updatedAskState(askState, { status: "cancelled", finishedAt: Date.now() })
    bumpAnswers()
  }

  function openAskInOpencode() {
    if (!askState.sessionId) return
    Quickshell.execDetached(["xdg-terminal-exec", "opencode", "--session",
      askState.sessionId, askState.workspace || homeDir])
  }

  function copyText(text, label) {
    var value = String(text || "")
    if (!value) return
    Quickshell.execDetached(["wl-copy", "--", value])
    Quickshell.execDetached(["notify-send", "-a", "Everything", "-t", "1800",
      "Copied " + String(label || "").slice(0, 80)])
  }

  Process {
    id: askProcess
    stdout: SplitParser {
      onRead: function(line) { root.askState = Answers.applyAskEvent(root.askState, line) }
    }
    stderr: StdioCollector { id: askStderr; waitForEnd: true }
    onExited: function(exitCode, exitStatus) {
      askTimeout.stop()
      if (root.askState.status !== "running") return
      var current = root.askState
      var failed = exitCode !== 0 && !current.answer
      var next = Answers.updatedAskState(current, {
        finishedAt: Date.now(),
        status: failed ? "error" : "done",
        error: failed ? (current.error || String(askStderr.text || "").trim().split("\n").pop().slice(0, 300)
          || ("opencode exited with " + exitCode)) : current.error
      })
      root.askState = next
      root.lastAsk = { prompt: next.prompt }
      root.bumpAnswers()
      // Nobody is looking: deliver the answer as a notification instead.
      if (root.leaseCount === 0 && next.status === "done") {
        Quickshell.execDetached(["notify-send", "-a", "opencode", "-t", "12000",
          next.prompt.slice(0, 80), next.answer.slice(0, 400)])
      }
    }
  }

  Timer {
    id: askTimeout
    interval: 300000
    onTriggered: {
      if (!askProcess.running) return
      askProcess.running = false
      root.askState = Answers.updatedAskState(root.askState,
        { status: "error", error: "Timed out after 5 minutes", finishedAt: Date.now() })
      root.bumpAnswers()
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
