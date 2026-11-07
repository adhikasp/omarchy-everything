import QtQuick
import Quickshell
import qs.Commons
import qs.Commons as Commons

// Demo stage for README media. Runs the real Everything panel against the
// fictional data in demo/ (see record.sh), types each scene, and saves the
// card with grabToImage. It only ever captures its own card item, never the
// screen, so nothing from the real desktop can leak into the media.
ShellRoot {
  id: root

  readonly property string sourceDir: Quickshell.env("EVERYTHING_SOURCE_DIR")
  readonly property string outDir: Quickshell.env("DEMO_OUT")
  property var service: null
  property var widget: null
  property var search: null
  property var card: null
  property int frame: 0
  property int stepIndex: 0
  property var steps: []

  function component(fileName) {
    return Qt.createComponent("file://" + sourceDir + "/" + fileName, Component.PreferSynchronous)
  }

  function findNamed(object, name) {
    if (!object) return null
    if (String(object.objectName || "") === name) return object
    var lists = [object.children || [], object.data || []]
    if (object.contentItem) lists.push([object.contentItem])
    for (var l = 0; l < lists.length; l++) {
      for (var i = 0; i < lists[l].length; i++) {
        var match = findNamed(lists[l][i], name)
        if (match) return match
      }
    }
    return null
  }

  // --- script ------------------------------------------------------------

  function typed(text, hold, still) {
    var out = [{ text: "", grab: false, wait: 220 }]
    for (var i = 1; i <= text.length; i++) {
      var last = i === text.length
      out.push({ text: text.slice(0, i), grab: true, wait: 150,
        duration: last ? hold : 95, still: last ? still : "" })
    }
    return out
  }

  function askRunning(prompt, seconds, tool) {
    return { action: function() {
      var started = Date.now() - seconds * 1000
      root.service.askState = { status: "running", prompt: prompt, workspace: "/home/demo/acme-api",
        answer: "", parts: [], sessionId: "ses_demo", tool: tool || "", error: "",
        startedAt: started, finishedAt: 0, cost: 0 }
      root.widget.askClock = Date.now()
    }, grab: true, wait: 220, duration: 650 }
  }

  function askDone(prompt) {
    return { action: function() {
      var started = Date.now() - 4200
      root.service.askState = { status: "done", prompt: prompt, workspace: "/home/demo/acme-api",
        answer: "The checkout test races the **payment webhook**: it asserts on the order before "
          + "`process_webhook` commits.\n\n"
          + "1. Await the `OrderPaid` event instead of sleeping 200 ms.\n"
          + "2. Use the test clock in `checkout/tests/flow.rs` so retries are deterministic.\n\n"
          + "Want me to open a branch with the fix?",
        parts: [], sessionId: "ses_demo", tool: "", error: "",
        startedAt: started, finishedAt: started + 4200, cost: 0.0021 }
    }, grab: true, wait: 300, duration: 3400, still: "ask" }
  }

  function buildSteps() {
    var ask = "why is the checkout test flaky?"
    var s = [{ open: true, grab: true, wait: 900, duration: 1700, still: "overview" }]
    s = s.concat(typed("auth", 1500, "search"))
    s = s.concat(typed("100 usd", 1700, "convert"))
    s = s.concat(typed("3pm tokyo", 1500, "time"))
    s = s.concat(typed("15% of 2400", 1300, "calc"))
    s = s.concat(typed("timer 25m tea", 1400, "timer"))
    s = s.concat(typed("clip", 1700, "clipboard"))
    s = s.concat(typed("? " + ask, 900, ""))
    s.push(askRunning(ask, 1, ""))
    s.push(askRunning(ask, 2, "read"))
    s.push(askRunning(ask, 3, "grep"))
    s.push(askDone(ask))
    // Stills only (not part of the GIF): the group checklist.
    s.push({ text: "", wait: 200 })
    s.push({ action: function() { root.widget.togglePopupMode(root.widget.groupsMode) },
      grab: true, wait: 700, duration: 0, still: "settings", stillOnly: true })
    return s
  }

  function runStep() {
    if (stepIndex >= steps.length) {
      console.log("DEMO_DONE")
      Qt.quit()
      return
    }
    var step = steps[stepIndex]
    if (step.open) widget.open()
    if (step.text !== undefined) search.text = step.text
    if (step.action) step.action()
    stepTimer.interval = step.wait || 150
    stepTimer.restart()
  }

  function afterWait() {
    var step = steps[stepIndex]
    stepIndex += 1
    if (!step.grab) { runStep(); return }
    var dpr = 2
    var name = "f" + String(frame).padStart(4, "0") + ".png"
    frame += 1
    card.grabToImage(function(result) {
      result.saveToFile(root.outDir + "/" + name)
      console.log("DEMO_FRAME " + JSON.stringify({ file: name, duration: step.duration || 0,
        still: step.still || "", stillOnly: step.stillOnly === true,
        width: Math.round(card.width * dpr), height: Math.round(card.height * dpr) }))
      root.runStep()
    })
  }

  Timer { id: stepTimer; repeat: false; onTriggered: root.afterWait() }

  // --- wiring ------------------------------------------------------------

  function start() {
    var serviceComponent = component("Service.qml")
    if (serviceComponent.status !== Component.Ready) {
      console.error("DEMO_ERROR service: " + serviceComponent.errorString()); Qt.quit(); return
    }
    service = serviceComponent.createObject(host, { shell: mockShell, omarchyPath: Quickshell.env("OMARCHY_PATH") })
    var widgetComponent = component("Everything.qml")
    if (widgetComponent.status !== Component.Ready) {
      console.error("DEMO_ERROR widget: " + widgetComponent.errorString()); Qt.quit(); return
    }
    widget = widgetComponent.createObject(barAnchor, { bar: mockBar, moduleName: "b.everything",
      settings: { id: "b.everything", askWorkspace: "~/acme-api", currencies: ["EUR", "GBP", "JPY"] } })
    search = findNamed(widget, "everything-search")
    var popup = findNamed(widget, "everything-popup")
    card = findNamed(popup, "everything-card")
    if (!search || !popup || !card) {
      console.error("DEMO_ERROR controls not found " + !!search + " " + !!popup + " " + !!card)
      Qt.quit(); return
    }
    steps = buildSteps()
    // Let the fake helper's snapshot and the zone offsets arrive first.
    service.acquire("demo")
    service.ensureZones()
    service.ensureRates()
    startTimer.start()
  }

  Timer { id: startTimer; interval: 1500; onTriggered: root.runStep() }
  Timer { interval: 1; running: true; onTriggered: root.start() }

  Item { id: host }

  QtObject {
    id: mockShell
    function serviceFor(id) { return id === "b.everything" ? root.service : null }
    function updateEntryInline(_moduleName, _settings) { return true }
  }

  // A bare-bones bar: the panel is screen-centred and never hangs from it.
  PanelWindow {
    id: barWindow
    anchors { top: true; left: true; right: true }
    implicitHeight: 1
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    Item { id: barAnchor; width: 1; height: 1 }
  }

  QtObject {
    id: mockBar
    property var shell: mockShell
    property string position: "top"
    property string fontFamily: "sans-serif"
    property color foreground: Commons.Color.foreground
    property color barForeground: Commons.Color.foreground
    property color urgent: Commons.Color.urgent
    property bool vertical: false
    property bool foregroundAnimationEnabled: false
    property int barSize: 1
    property var activePopout: null
    property var clickTargets: []
    function registerClickTarget(target) { clickTargets = clickTargets.concat([target]) }
    function unregisterClickTarget(target) { clickTargets = clickTargets.filter(function(v) { return v !== target }) }
    function requestPopout(owner) { activePopout = owner }
    function releasePopout(owner) { if (activePopout === owner) activePopout = null }
    function switchPanelFrom(_owner, _direction) { return false }
    function showTooltip(_target, _text) {}
    function hideTooltip(_target) {}
  }
}
