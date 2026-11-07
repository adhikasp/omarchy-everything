pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "EverythingModel.js" as Model
import "InstantAnswers.js" as Answers

Panel {
  id: root
  moduleName: "io.github.adhikasp.everything"
  ipcTarget: "io.github.adhikasp.everything"
  manageIpc: false

  // Hyprland delivers this straight into the running shell. The IPC route
  // (`omarchy-shell shell toggle`) spawns a whole Qt process, ~55 ms per press.
  GlobalShortcut {
    appid: "omarchy-everything"
    name: "toggle"
    description: "Toggle Everything"
    onPressed: { if (!root.opened) root.captureFocusContext(); root.toggle() }
  }

  readonly property var everythingService: root.bar && root.bar.shell
    && typeof root.bar.shell.serviceFor === "function"
    ? root.bar.shell.serviceFor("io.github.adhikasp.everything") : null
  readonly property var appLibrary: root.bar && root.bar.shell
    && root.bar.shell.appLibrary !== undefined ? root.bar.shell.appLibrary : null
  readonly property var desktopEntries: DesktopEntries.applications.values || []
  readonly property var itemRelations: Model.relationIndex(
    everythingService ? everythingService.items : [])
  readonly property var supportedGroups: Model.supportedGroupCatalog()
  readonly property var disabledKinds: Model.normalizeDisabledKinds(
    root.setting("disabledKinds", []))
  readonly property var checklistEntries: supportedGroups
  // Same palette as Omarchy's own launcher ([menu] in shell.toml), so the
  // card reads as part of the OS on every theme. Secondary text is an alpha
  // of the foreground, never Qt.darker(): on a light theme "darker" is louder.
  readonly property color foreground: Commons.Color.menu.text
  readonly property color dimForeground: Util.alpha(foreground, 0.6)
  readonly property color faintForeground: Util.alpha(foreground, 0.4)
  readonly property color hairlineColor: Util.alpha(foreground, 0.1)
  readonly property color selectedFill: Commons.Color.menu.selectedBackground
  readonly property color selectedText: Commons.Color.menu.selectedText
  readonly property color accentColor: Commons.Color.accent
  readonly property string matchColor: String(selectedText)
  // Launcher text runs larger than the OS default (user ask, 2026-11-04).
  readonly property real fontScale: 1.5
  // Words use a proportional UI face; the Nerd Font (Style.font.family)
  // stays for icon glyphs and key caps. Inter when installed, else Noto Sans.
  readonly property string uiFamily: {
    var families = Qt.fontFamilies()
    var wanted = ["Inter", "Inter Variable", "Noto Sans"]
    for (var i = 0; i < wanted.length; i++)
      if (families.indexOf(wanted[i]) >= 0) return wanted[i]
    return "sans-serif"
  }
  readonly property int titlePx: Math.round(Style.font.body * fontScale)
  readonly property int searchPx: Math.round(Style.font.heading * fontScale)
  readonly property int metaPx: Math.round(Style.font.bodySmall * 1.3)
  readonly property int iconPx: Math.round(Style.font.icon * fontScale)
  readonly property int sectionPx: Math.round(Style.font.caption * 1.2)
  readonly property int hintPx: Math.round(Style.font.caption * 1.25)
  readonly property real rowHeight: Math.round(titlePx * 2.1)
  readonly property int sectionHeight: Style.space(30)
  readonly property int searchHeight: Math.round(searchPx * 2.3)
  readonly property int rowInset: Style.space(10)
  readonly property int iconBox: Style.space(30)
  readonly property int groupChecklistRowHeight: Math.round(titlePx * 2.1)
  readonly property bool listMode: resultList.activeFocus || keyCatcher.activeFocus
  // Typing always re-targets the best match; a background refresh keeps the
  // row you were on. "Typing" means the query differs from the last ranked one.
  property string lastRankedQuery: ""
  readonly property string resultsMode: "results"
  readonly property string groupsMode: "groups"
  readonly property bool resultsModeOpen: root.opened && popupMode === resultsMode
  property var rankedItems: []
  property var collapsedKinds: ({})
  property string popupMode: resultsMode
  property string selectedItemUuid: ""
  property real savedSelectedOffset: 0
  property bool savedListFocus: false
  property bool savedKeyCatcherFocus: false
  property bool savedSearchFocus: false
  property bool pendingSelectionSurvived: false
  property bool resultUpdateInProgress: false
  property bool rankUpdateScheduled: false
  property int resultUpdateSerial: 0
  property bool pointerCursorActive: false
  // A widget may acquire its service lease before its QsWindow/screen exists.
  // Keep the owner key stable and instance-local so an early "unknown" screen
  // can never collide with another monitor or leak a lease after reparenting.
  readonly property string instanceKey: "everything:" + Date.now() + ":"
    + Math.random().toString(36).slice(2)

  function disarmPointer() {
    pointerCursorActive = false
    pointerMoveGate.reset()
  }

  function kindDisabled(kind) {
    return disabledKinds.indexOf(String(kind || "")) >= 0
  }

  function persistDisabledKinds(value) {
    var nextKinds = Model.normalizeDisabledKinds(value)
    if (Model.sameStringList(disabledKinds, nextKinds)) return

    persistSetting("disabledKinds", nextKinds)
  }

  function persistSetting(name, value) {
    var entry = { id: root.moduleName }
    for (var key in root.settings) if (key !== "id") entry[key] = root.settings[key]
    entry[name] = value

    // Update this monitor immediately. The shell write then injects the same
    // settings object into every live monitor instance without rebuilding it.
    root.settings = entry
    if (root.bar && root.bar.shell
        && typeof root.bar.shell.updateEntryInline === "function")
      root.bar.shell.updateEntryInline(root.moduleName, entry)
  }

  function setKindEnabled(kind, enabled) {
    var value = String(kind || "")
    var shouldDisable = enabled !== true
    var catalogKind = false
    for (var index = 0; index < supportedGroups.length; index++) {
      if (supportedGroups[index].kind === value) {
        catalogKind = true
        break
      }
    }
    if (!catalogKind || kindDisabled(value) === shouldDisable) return

    var nextKinds = disabledKinds.slice()
    var disabledIndex = nextKinds.indexOf(value)
    if (!shouldDisable && disabledIndex >= 0) nextKinds.splice(disabledIndex, 1)
    else if (shouldDisable && disabledIndex < 0) nextKinds.push(value)
    persistDisabledKinds(nextKinds)
  }

  function toggleKindVisibility(kind) {
    setKindEnabled(kind, kindDisabled(kind))
  }

  function toggleChecklistEntry(entry) {
    toggleKindVisibility(entry.kind)
  }

  function resetGroupChecklist() {
    groupList.currentIndex = checklistEntries.length > 0 ? 0 : -1
    groupList.contentY = groupList.originY
    Qt.callLater(function() {
      if (root.opened && root.popupMode === root.groupsMode)
        groupList.forceActiveFocus()
    })
  }

  function enterPopupMode(mode) {
    var wanted = mode === groupsMode ? groupsMode : resultsMode
    if (popupMode === wanted) return
    popupMode = wanted
    if (!root.opened) return

    if (wanted === groupsMode) {
      resetGroupChecklist()
    } else {
      searchField.text = ""
      if (appLibrary && typeof appLibrary.refreshIcons === "function")
        appLibrary.refreshIcons()
      rebuildRankedItems()
      resetResultsForOpen()
    }
  }

  function togglePopupMode(mode) {
    var wanted = mode === groupsMode ? groupsMode : resultsMode
    if (root.opened && popupMode === wanted) {
      root.close()
      return
    }
    if (root.opened) {
      enterPopupMode(wanted)
      return
    }
    popupMode = wanted
    captureFocusContext()
    root.open()
  }

  function setGroupChecklistIndex(index) {
    if (checklistEntries.length <= 0) {
      groupList.currentIndex = -1
      return
    }
    var bounded = Math.max(0, Math.min(checklistEntries.length - 1,
      Math.floor(Number(index || 0))))
    groupList.currentIndex = bounded
    groupList.positionViewAtIndex(bounded, ListView.Contain)
  }

  function moveGroupChecklist(delta) {
    if (checklistEntries.length <= 0 || Number(delta || 0) === 0) return
    var current = groupList.currentIndex < 0 ? 0 : groupList.currentIndex
    setGroupChecklistIndex(current + (delta < 0 ? -1 : 1))
    groupList.forceActiveFocus()
  }

  function toggleCurrentGroupVisibility() {
    var index = groupList.currentIndex
    if (index < 0 || index >= checklistEntries.length) return
    toggleChecklistEntry(checklistEntries[index])
  }

  function selectFromPointer(index, item, mouse) {
    if (!pointerMoveGate.moved(item, mouse)) return
    setCurrentIndex(index, false)
    pointerCursorActive = true
  }

  function focusSearch() {
    disarmPointer()
    searchField.forceActiveFocus()
  }

  function kindCount(kind) {
    var total = 0
    var rows = rankedItems
    for (var i = 0; i < rows.length; i++)
      if (String(rows[i].kind || "") === String(kind || "")) total += 1
    return total
  }

  function groupCollapsed(kind) {
    return collapsedKinds && collapsedKinds[String(kind || "")] === true
  }

  function currentGroupKind() {
    var index = resultList.currentIndex
    if (index < 0 || index >= rankedItems.length) return ""
    return String(rankedItems[index] && rankedItems[index].kind || "")
  }

  function selectionIsVisibleRow(index) {
    return index >= 0 && index < rankedItems.length
      && !groupCollapsed(rankedItems[index].kind)
  }

  function focusList(index) {
    if (resultList.count <= 0) return
    disarmPointer()
    setCurrentIndex(index === undefined ? 0 : index, true)
    if (resultList.currentIndex === 0)
      resultList.contentY = resultList.originY
    resultList.forceActiveFocus()
  }

  // While the card grows on open, ListView's follow-scroll keeps the first
  // row in view but not its group heading, which then sits above the fold.
  // For a moment after each open, a first-row selection rests at the top.
  function pinTopIfFirst() {
    // A background refresh restores its own scroll position; the index is
    // transiently 0 while it does, which must not read as "first row".
    // Only while typing: in browse mode the user owns the scroll position.
    if (resultList.count <= 0 || resultUpdateInProgress || !searchField.activeFocus) return
    if (resultList.currentIndex === Model.boundaryNavigableIndex(rankedItems, -1, collapsedKinds)
        && resultList.contentY !== resultList.originY)
      resultList.contentY = resultList.originY
  }

  Timer {
    id: openPinTimer
    property int ticks: 0
    interval: 60
    repeat: true
    onTriggered: {
      root.pinTopIfFirst()
      ticks += 1
      if (ticks >= 8) stop()
    }
  }

  function setCurrentIndex(index, reveal) {
    if (resultList.count <= 0 || rankedItems.length <= 0) {
      resultList.currentIndex = -1
      selectedItemUuid = ""
      return
    }
    var bounded = Math.max(0, Math.min(Number(index || 0), resultList.count - 1))
    resultList.currentIndex = bounded
    selectedItemUuid = Model.itemUuid(rankedItems[bounded])
    if (reveal === true) revealSelection(bounded, false)
  }

  function moveSelection(delta, keepSearchFocus) {
    if (resultList.count <= 0 || Number(delta || 0) === 0) return
    var direction = delta < 0 ? -1 : 1
    var steps = Math.max(1, Math.floor(Math.abs(Number(delta))))
    var candidate = resultList.currentIndex
    if (candidate < 0)
      candidate = Model.boundaryNavigableIndex(rankedItems, direction, collapsedKinds)
    for (var step = 0; step < steps; step++) {
      var next = Model.adjacentNavigableIndex(
        rankedItems, candidate, direction, collapsedKinds)
      if (next === candidate) break
      candidate = next
    }
    if (keepSearchFocus === true) {
      // Launcher behaviour: arrows move the highlight, typing keeps typing.
      disarmPointer()
      setCurrentIndex(candidate, true)
      if (candidate === Model.boundaryNavigableIndex(rankedItems, -1, collapsedKinds))
        resultList.contentY = resultList.originY
      return
    }
    focusList(candidate)
  }

  function focusBoundary(direction) {
    focusList(Model.boundaryNavigableIndex(rankedItems, direction, collapsedKinds))
  }

  function setGroupCollapsed(kind, collapsed) {
    var value = String(kind || "")
    var wanted = collapsed === true
    if (!value || groupCollapsed(value) === wanted) return

    var nextKinds = {}
    for (var key in collapsedKinds) {
      if (collapsedKinds[key] === true) nextKinds[key] = true
    }
    if (wanted) nextKinds[value] = true
    else delete nextKinds[value]
    collapsedKinds = nextKinds
    disarmPointer()
    var index = resultList.currentIndex
    if (typeof resultList.forceLayout === "function") resultList.forceLayout()
    revealSelection(index, wanted)
  }

  function collapseCurrentGroup() {
    var kind = currentGroupKind()
    if (kind) setGroupCollapsed(kind, true)
  }

  function expandCurrentGroup() {
    var kind = currentGroupKind()
    if (kind) setGroupCollapsed(kind, false)
  }

  function toggleGroupFromHeading(kind) {
    var index = -1
    for (var itemIndex = 0; itemIndex < rankedItems.length; itemIndex++) {
      if (String(rankedItems[itemIndex].kind || "") === String(kind || "")) {
        index = itemIndex
        break
      }
    }
    if (index < 0) return
    setCurrentIndex(index, false)
    resultList.forceActiveFocus()
    setGroupCollapsed(kind, !groupCollapsed(kind))
  }

  function activateAt(index) {
    if (index < 0 || index >= rankedItems.length || !everythingService
        || !selectionIsVisibleRow(index)) return
    var target = rankedItems[index]
    var local = target.local
    if (Answers.isAnswerKind(target.kind)) {
      if (!local) return
      if (local.type === "ask") {
        everythingService.ask(local.prompt, root.askWorkspace, root.askModel)
        return
      }
      if (local.type === "query") {
        searchField.text = String(local.text || "")
        root.focusSearch()
        return
      }
    }
    // Release the layer-shell keyboard grab before asking another surface to
    // accept focus. Service.qml keeps the helper alive for the pending action.
    root.close()
    Qt.callLater(function() { everythingService.activate(target) })
  }

  function activateCurrent() {
    if (groupCollapsed(currentGroupKind())) {
      expandCurrentGroup()
      return
    }
    activateAt(resultList.currentIndex)
  }

  function hideAt(index) {
    if (index < 0 || index >= rankedItems.length || !everythingService
        || !selectionIsVisibleRow(index)) return
    everythingService.hideItem(rankedItems[index].id)
  }

  function handleEscape() {
    if (searchField.text.length > 0) {
      searchField.text = ""
      focusSearch()
    } else {
      root.close()
    }
  }

  function rowTopAt(index) {
    return Model.sectionedRowTop(
      rankedItems, index, rowHeight, sectionHeight, collapsedKinds)
  }

  function rowVisualTop(index) {
    if (index >= 0 && typeof resultList.itemAtIndex === "function") {
      var delegateItem = resultList.itemAtIndex(index)
      if (delegateItem) return Number(delegateItem.y)
    }
    return resultList.originY + rowTopAt(index)
  }

  function selectionVisualTop(index) {
    var top = rowVisualTop(index)
    if (index >= 0 && index < rankedItems.length
        && groupCollapsed(rankedItems[index].kind)) top -= sectionHeight
    return top
  }

  function revealSelection(index, alignTop) {
    if (index < 0 || index >= rankedItems.length) return
    if (typeof resultList.forceLayout === "function") resultList.forceLayout()
    var top = selectionVisualTop(index)
    var extent = groupCollapsed(rankedItems[index].kind) ? sectionHeight : rowHeight
    var wanted = resultList.contentY
    if (alignTop === true || top < wanted) wanted = top
    else if (top + extent > wanted + resultList.height)
      wanted = top + extent - resultList.height
    resultList.contentY = Model.clampContentY(
      wanted, resultList.originY, resultList.contentHeight, resultList.height)
  }

  function pageDelta(direction) {
    if (resultList.count <= 0 || direction === 0) return 0
    var current = Math.max(0, Math.min(resultList.count - 1, resultList.currentIndex))
    var target = selectionVisualTop(current)
      + (direction > 0 ? resultList.height : -resultList.height)
    var candidate = current
    var steps = 0
    while (true) {
      var next = Model.adjacentNavigableIndex(
        rankedItems, candidate, direction, collapsedKinds)
      if (next === candidate) break
      var nextTop = selectionVisualTop(next)
      if ((direction > 0 && nextTop > target)
          || (direction < 0 && nextTop < target)) break
      candidate = next
      steps++
    }
    if (steps === 0 && Model.adjacentNavigableIndex(
        rankedItems, candidate, direction, collapsedKinds) !== candidate) steps = 1
    return direction > 0 ? steps : -steps
  }

  function rememberFocusState() {
    savedKeyCatcherFocus = keyCatcher.activeFocus
    savedListFocus = resultList.activeFocus || savedKeyCatcherFocus
    savedSearchFocus = searchField.activeFocus
  }

  function rememberResultState() {
    rememberFocusState()
    var current = resultList.currentIndex
    if (current >= 0 && current < rankedItems.length) {
      selectedItemUuid = Model.itemUuid(rankedItems[current])
    }
    savedSelectedOffset = current >= 0
      ? resultList.contentY - selectionVisualTop(current) : 0
  }

  function applyOpenResultReset() {
    var index = rankedItems.length > 0 ? 0 : -1
    resultList.currentIndex = index
    selectedItemUuid = index >= 0 ? Model.itemUuid(rankedItems[index]) : ""
    savedSelectedOffset = 0
    resultList.contentY = resultList.originY
  }

  function resetResultsForOpen() {
    // A restore queued during the previous opening must never override the
    // fresh-open state after the panel becomes visible again.
    disarmPointer()
    resultUpdateSerial += 1
    collapsedKinds = ({})
    resultUpdateInProgress = false
    pendingSelectionSurvived = false
    applyOpenResultReset()
    openPinTimer.ticks = 0
    openPinTimer.restart()
    Qt.callLater(function() {
      if (!root.opened) return
      if (typeof resultList.forceLayout === "function") resultList.forceLayout()
      root.applyOpenResultReset()
      searchField.forceActiveFocus()
    })
  }

  function scheduleRankedItems() {
    if (rankUpdateScheduled) return
    rankUpdateScheduled = true
    Qt.callLater(function() {
      root.rankUpdateScheduled = false
      root.rebuildRankedItems()
    })
  }

  // --- instant answers and ask -------------------------------------------

  readonly property string askWorkspace: String(root.setting("askWorkspace", "~"))
  readonly property string askModel: String(root.setting("askModel", ""))
  readonly property string askLabel: askWorkspace === "~" ? "home"
    : askWorkspace.replace(/\/+$/, "").split("/").pop()
  readonly property var askState: everythingService ? everythingService.askState : Answers.emptyAskState()
  readonly property bool askRunning: askState.status === "running"
  // The answer view is up while the query is exactly the prompt that was asked.
  readonly property bool answerMode: popupMode === resultsMode && askState.status !== ""
    && askPromptOf(searchField.text) === askState.prompt
  property real askClock: Date.now()

  function askPromptOf(text) {
    var m = /^\s*(?:\?|ask\s)\s*([\s\S]*)$/i.exec(String(text || ""))
    return m ? m[1].trim() : null
  }

  function answerContext() {
    var svc = everythingService
    var currencies = root.setting("currencies", ["USD", "EUR"])
    return {
      now: Date.now(),
      disabledKinds: disabledKinds,
      rates: svc ? svc.rates : null,
      ratesError: svc ? svc.ratesError : "",
      // Settings can arrive as a QVariantList, which is array-like, not an Array.
      currencies: currencies && currencies.length > 0
        ? Array.prototype.slice.call(currencies).map(String) : ["USD", "EUR"],
      tzOffsets: svc ? svc.tzOffsets : null,
      localZone: svc ? svc.localZone : "UTC",
      clipboard: svc ? svc.clipboardHistory : null,
      home: svc ? svc.homeDir : "",
      reminders: svc ? svc.reminders : [],
      askLabel: askLabel,
      lastAsk: svc ? svc.lastAsk : null
    }
  }

  function currentAnswerAction() {
    var index = resultList.currentIndex
    if (index < 0 || index >= rankedItems.length) return ""
    var local = rankedItems[index].local
    if (!local) return ""
    var labels = { copy: "copy", "paste-clip": "paste", "paste-image": "paste",
      ask: "ask", query: "show", exec: "start" }
    if (local.type === "exec" && rankedItems[index].kind === "timer"
        && String(rankedItems[index].id).indexOf("clear") >= 0) return "clear"
    return labels[local.type] || ""
  }

  // Hint and status rows (e.g. "No reminders running") do nothing on Enter.
  function currentRowInert() {
    var index = resultList.currentIndex
    if (index < 0 || index >= rankedItems.length) return false
    var item = rankedItems[index]
    return Answers.isAnswerKind(item.kind) && !item.local
  }

  function copyAnswer() {
    if (!everythingService || !askState.answer) return
    everythingService.copyText(askState.answer, "answer")
    root.close()
  }

  function scrollAnswer(delta) {
    var target = answerFlick.contentY + delta
    answerFlick.contentY = Math.max(0, Math.min(target,
      Math.max(0, answerFlick.contentHeight - answerFlick.height)))
  }

  Timer {
    interval: 1000
    repeat: true
    running: root.askRunning && root.opened
    onTriggered: root.askClock = Date.now()
  }

  // Window class that had focus when the panel opened; leads the empty-query list.
  property string focusLeadKind: ""

  function captureFocusContext() {
    var top = ToplevelManager.activeToplevel
    var cls = top ? String(top.appId || "") : ""
    focusLeadKind = Model.contextLeadKind(cls)
  }

  function rebuildRankedItems() {
    var nextItems = Model.rank(
      everythingService ? everythingService.items : [],
      searchField.text,
      everythingService ? everythingService.hiddenIds : {},
      disabledKinds,
      focusLeadKind)
    // Instant answers pin above every search result; prefix modes (ask,
    // clipboard, timer) replace the search results entirely.
    var answers = Answers.instantAnswers(searchField.text, answerContext())
    if (everythingService && answers.needs) everythingService.ensureAnswerData(answers.needs)
    nextItems = answers.exclusive ? answers.items : answers.items.concat(nextItems)
    var retarget = searchField.text !== lastRankedQuery
    lastRankedQuery = searchField.text
    if (Model.sameRankedItems(rankedItems, nextItems)) {
      if (retarget && rankedItems.length > 0 && !resultUpdateInProgress) {
        setCurrentIndex(0, false)
        resultList.contentY = resultList.originY
      }
      return
    }
    if (!resultUpdateInProgress) rememberResultState()
    if (retarget) {
      selectedItemUuid = ""
      savedSelectedOffset = 0
    }
    disarmPointer()
    resultUpdateInProgress = true
    rankedItems = nextItems
    resultUpdateSerial += 1
    var serial = resultUpdateSerial
    Qt.callLater(function() {
      if (serial === root.resultUpdateSerial) root.restoreResultSelection(serial)
    })
  }

  function restoreResultSelection(serial) {
    if (serial !== resultUpdateSerial) return
    if (typeof resultList.forceLayout === "function") resultList.forceLayout()
    pendingSelectionSurvived = Model.indexOfUuid(rankedItems, selectedItemUuid) >= 0
    var index = Model.indexAfterRefresh(rankedItems, selectedItemUuid)
    resultList.currentIndex = index
    selectedItemUuid = index >= 0 ? Model.itemUuid(rankedItems[index]) : ""
    if (!pendingSelectionSurvived) resultList.contentY = resultList.originY
    Qt.callLater(function() {
      if (serial === root.resultUpdateSerial) root.finishResultRestore(serial)
    })
  }

  function finishResultRestore(serial) {
    if (serial !== resultUpdateSerial) return
    if (typeof resultList.forceLayout === "function") resultList.forceLayout()
    var index = Model.indexOfUuid(rankedItems, selectedItemUuid)
    if (index < 0) index = rankedItems.length > 0 ? 0 : -1
    resultList.currentIndex = index
    selectedItemUuid = index >= 0 ? Model.itemUuid(rankedItems[index]) : ""

    if (pendingSelectionSurvived && index >= 0
        && resultList.contentHeight > resultList.height + 0.5) {
      resultList.contentY = Model.clampContentY(selectionVisualTop(index) + savedSelectedOffset,
        resultList.originY, resultList.contentHeight, resultList.height)
    } else if (!pendingSelectionSurvived) {
      resultList.contentY = resultList.originY
    }

    if (savedKeyCatcherFocus) keyCatcher.forceActiveFocus()
    else if (savedListFocus && index >= 0) resultList.forceActiveFocus()
    else if (savedSearchFocus) searchField.forceActiveFocus()
    resultUpdateInProgress = false
  }

  function breadcrumb(item) {
    return Model.breadcrumbFromIndex(item, itemRelations)
  }

  function usableIconSource(value) {
    var source = String(value || "")
    var generic = String(Quickshell.iconPath("application-x-executable", true) || "")
    return source && source !== generic ? source : ""
  }

  function hasIconHints(item) {
    var kind = String(item && item.kind || "")
    return kind === "window" || kind === "app" || kind === "browser-tab"
  }

  function resolveWindowDesktopEntry(item) {
    if (!hasIconHints(item)) return ""
    var hints = Model.iconHintList(item)
    if (hints.length === 0) return null

    var entry = Model.matchDesktopEntry(hints, desktopEntries)
    if (!entry) {
      for (var lookup = 0; lookup < hints.length && !entry; lookup++) {
        try { entry = DesktopEntries.heuristicLookup(hints[lookup]) } catch (_error) { }
      }
    }
    return entry || null
  }

  function windowIconSource(item, resolvedEntry) {
    if (!hasIconHints(item)) return ""
    var hints = Model.iconHintList(item)
    if (hints.length === 0) return ""

    var entry = resolvedEntry || resolveWindowDesktopEntry(item)
    if (entry && entry.icon) {
      if (appLibrary && typeof appLibrary.iconSource === "function") {
        var librarySource = usableIconSource(appLibrary.iconSource(entry.icon))
        if (librarySource) return librarySource
      }
      var entrySource = usableIconSource(Quickshell.iconPath(String(entry.icon), true))
      if (entrySource) return entrySource
    }

    // A direct icon name from Hyprland can be useful even when it has no
    // corresponding desktop entry. Unknown names remain a local glyph
    // fallback; they are never treated as arbitrary URLs.
    for (var hint = 0; hint < hints.length; hint++) {
      var directSource = usableIconSource(Quickshell.iconPath(hints[hint], true))
      if (directSource) return directSource
    }
    return ""
  }

  function handleSupplementalListKey(event) {
    var ctrl = (event.modifiers & Qt.ControlModifier) !== 0
    var action = ""
    if (ctrl && event.key === Qt.Key_N) action = "next"
    else if (ctrl && event.key === Qt.Key_P) action = "previous"
    else if (event.key === Qt.Key_Home) action = "first"
    else if (event.key === Qt.Key_End) action = "last"
    else if (event.key === Qt.Key_PageDown || (ctrl && event.key === Qt.Key_D)) action = "page-next"
    else if (event.key === Qt.Key_PageUp || (ctrl && event.key === Qt.Key_U)) action = "page-previous"
    else if (event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) action = "hide"

    if (!action) return
    event.accepted = true
    if (action === "next") moveSelection(1)
    else if (action === "previous") moveSelection(-1)
    else if (action === "first") focusBoundary(-1)
    else if (action === "last") focusBoundary(1)
    else if (action === "page-next") moveSelection(pageDelta(1))
    else if (action === "page-previous") moveSelection(pageDelta(-1))
    else if (action === "hide") hideAt(resultList.currentIndex)
  }

  onOpenedChanged: {
    if (root.opened) {
      if (popupMode === groupsMode) {
        resetGroupChecklist()
      } else {
        searchField.text = ""
        if (appLibrary && typeof appLibrary.refreshIcons === "function")
          appLibrary.refreshIcons()
        rebuildRankedItems()
        resetResultsForOpen()
      }
    } else {
      popupMode = resultsMode
    }
  }

  onResultsModeOpenChanged: {
    if (!everythingService) return
    if (resultsModeOpen) everythingService.acquire(instanceKey)
    else everythingService.release(instanceKey)
  }

  onEverythingServiceChanged: {
    if (resultsModeOpen && everythingService)
      everythingService.acquire(instanceKey)
    scheduleRankedItems()
  }
  onDisabledKindsChanged: scheduleRankedItems()
  Component.onCompleted: scheduleRankedItems()
  Component.onDestruction: if (everythingService) everythingService.release(instanceKey)

  Connections {
    target: root.everythingService
    ignoreUnknownSignals: true
    function onItemsChanged() { root.scheduleRankedItems() }
    function onHiddenIdsChanged() { root.scheduleRankedItems() }
    function onAnswerRevisionChanged() { root.scheduleRankedItems() }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  PointerMoveGate {
    id: pointerMoveGate
    referenceItem: resultList
  }

  BarIconButton {
    id: button
    objectName: "everything-bar-button"
    anchors.fill: parent
    bar: root.bar
    text: "\uf078"
    useActiveColor: false
    iconComponent: Component {
      Item {
        id: chevronFrame
        objectName: "everything-bar-chevron"
        // QML pixel-aligns the even optical canvas inside the odd bar slot.
        // Counter that half-pixel shift without moving the slot or hit target.
        readonly property real buttonCenterX: button.width / 2
          - Math.round((button.width - width) / 2)
        readonly property real centerErrorX: chevron.x
          + chevron.paintedCenterX - buttonCenterX

        OpticalGlyph {
          id: chevron
          width: parent.width
          height: parent.height
          x: chevronFrame.buttonCenterX - paintedCenterX
          text: button.text
          fontFamily: button.fontFamily
          fontSize: button.fontSize
          color: button.active && button.useActiveColor
            ? button.activeColor : button.foreground
        }
      }
    }
    active: root.opened
    tooltipText: "Everything"
    activeFocusOnTab: true
    Keys.onReturnPressed: root.togglePopupMode(root.resultsMode)
    Keys.onEnterPressed: root.togglePopupMode(root.resultsMode)
    Keys.onSpacePressed: root.togglePopupMode(root.resultsMode)
    onPressed: function(buttonCode) {
      root.togglePopupMode(buttonCode === Qt.RightButton
        ? root.groupsMode : root.resultsMode)
    }

    Accessible.role: Accessible.Button
    Accessible.name: "Open Everything"
    Accessible.description: "Search things with the primary action; choose visible groups with right-click"
    Accessible.focusable: true
    Accessible.focused: activeFocus
    Accessible.onPressAction: root.togglePopupMode(root.resultsMode)

    Rectangle {
      anchors.fill: parent
      anchors.margins: Style.space(2)
      visible: button.activeFocus
      color: "transparent"
      radius: Style.cornerRadius
      border.width: Math.max(1, Style.normalBorderWidth)
      border.color: Commons.Color.accent
    }
  }

  readonly property var footerHints: popupMode === groupsMode
    ? [{ key: "↑↓", label: "move" }, { key: "↵", label: "toggle" },
       { key: "esc", label: "close" }]
    : answerMode
      ? (askRunning
        ? [{ key: "esc", label: "back" }]
        : [{ key: "↵", label: "copy" }, { key: "ctrl o", label: "open in opencode" },
           { key: "↑↓", label: "scroll" }, { key: "esc", label: "back" }])
    : rankedItems.length === 0
      ? [{ key: "esc", label: searchField.text.length > 0 ? "clear" : "close" }]
    : listMode
      ? [{ key: "↑↓", label: "move" }, { key: "←→", label: "fold" },
         { key: "↵", label: "open" }, { key: "del", label: "hide" },
         { key: "/", label: "search" }]
      : [{ key: "↑↓", label: "select" }]
          .concat(root.currentRowInert() ? [] : [{ key: "↵", label: root.currentAnswerAction() || "open" }])
          .concat([{ key: "tab", label: "browse" },
            { key: "esc", label: searchField.text.length > 0 ? "clear" : "close" }])

  readonly property int shownGroupCount: {
    var shown = 0
    for (var i = 0; i < checklistEntries.length; i++)
      if (!kindDisabled(checklistEntries[i].kind)) shown += 1
    return shown
  }

  // Small keyboard-key chip for the footer and the search line.
  component KeyCap: Rectangle {
    id: cap
    property string label: ""
    property color ink: Commons.Color.menu.text
    property int pixelSize: Style.font.caption
    implicitWidth: Math.max(implicitHeight, capText.implicitWidth + Style.space(10))
    implicitHeight: Math.round(pixelSize * 1.75)
    radius: Math.min(Style.cornerRadius, Style.space(4))
    color: Util.alpha(ink, 0.06)
    border.width: Style.spacing.hairline
    border.color: Util.alpha(ink, 0.16)

    Text {
      id: capText
      anchors.centerIn: parent
      textFormat: Text.PlainText
      text: cap.label
      color: Util.alpha(cap.ink, 0.7)
      font.family: Style.font.family
      font.pixelSize: cap.pixelSize
    }
  }

  CenteredPanel {
    id: popup
    objectName: "everything-popup"
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: root.popupMode === root.groupsMode ? groupList : searchField
    // Wide, centered card whose height follows its results (up to 60% of the
    // screen); the top edge stays put so typing never moves the search field.
    readonly property real chromeHeight: popup.verticalContentInset
      + root.searchHeight + Style.spacing.hairline + Style.space(6)
      + Style.space(6) + footer.implicitHeight
    readonly property real resultsDesiredHeight: chromeHeight
      + (root.rankedItems.length === 0 ? Style.space(150)
        : Math.max(resultList.contentHeight, root.rowHeight))
    readonly property real groupsDesiredHeight: chromeHeight + groupList.contentHeight
    readonly property real answerDesiredHeight: chromeHeight + Style.space(10)
      + answerHeader.height + Style.space(10) + answerFlick.contentHeight + Style.space(8)
    contentWidth: popup.fittedContentWidth(Style.space(780))
    contentHeight: root.popupMode === root.groupsMode
      ? popup.cappedContentHeight(Math.min(groupsDesiredHeight, popup.screenH * 0.6))
      : root.answerMode
      ? popup.cappedContentHeight(Math.max(Style.space(200),
          Math.min(answerDesiredHeight, popup.screenH * 0.66)))
      : popup.cappedContentHeight(Math.max(Style.space(160),
          Math.min(resultsDesiredHeight, popup.screenH * 0.6)))

    PanelKeyCatcher {
      id: keyCatcher
      objectName: "everything-key-catcher"
      anchors.fill: parent
      blocked: root.popupMode === root.resultsMode && searchField.activeFocus

      onMoveRequested: function(dx, dy) {
        if (root.popupMode === root.groupsMode) {
          if (dy !== 0) root.moveGroupChecklist(dy)
        } else if (dy !== 0) root.moveSelection(dy)
        else if (dx < 0) root.collapseCurrentGroup()
        else if (dx > 0) root.expandCurrentGroup()
      }
      onActivateRequested: {
        if (root.popupMode === root.groupsMode) root.toggleCurrentGroupVisibility()
        else root.activateCurrent()
      }
      onCloseRequested: {
        if (root.popupMode === root.groupsMode) root.close()
        else root.handleEscape()
      }
      onDeleteRequested: if (root.popupMode === root.resultsMode)
        root.hideAt(resultList.currentIndex)
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(text) {
        if (root.popupMode !== root.resultsMode) return
        if (text === "/") root.focusSearch()
        else if (text === "g") root.focusBoundary(-1)
        else if (text === "G") root.focusBoundary(1)
        else if (text.trim().length > 0) {
          // Any other printable key while browsing goes back to typing.
          searchField.text += text
          root.focusSearch()
        }
      }

      Item {
        id: content
        anchors.fill: parent
        Accessible.role: Accessible.Pane
        Accessible.name: root.popupMode === root.groupsMode
          ? "Everything group visibility panel" : "Everything search and switch panel"

      // --- header: search line ------------------------------------------

      Item {
        id: searchHeader
        objectName: "everything-search-header"
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: root.searchHeight
        visible: root.popupMode === root.resultsMode

        Text {
          id: searchGlyph
          anchors.left: parent.left
          anchors.leftMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          width: root.iconBox
          horizontalAlignment: Text.AlignHCenter
          textFormat: Text.PlainText
          text: ""
          color: searchField.text.length > 0 ? root.selectedText : root.faintForeground
          font.family: Style.font.family
          font.pixelSize: root.iconPx
        }

        TextField {
          id: searchField
          objectName: "everything-search"
          anchors.left: searchGlyph.right
          anchors.leftMargin: Style.space(10)
          anchors.right: clearHint.visible ? clearHint.left : parent.right
          anchors.rightMargin: Style.space(10)
          anchors.verticalCenter: parent.verticalCenter
          visible: root.popupMode === root.resultsMode
          enabled: visible
          placeholderText: "Search windows, tabs, apps, menu…"
          placeholderTextColor: root.faintForeground
          foreground: root.foreground
          accent: root.accentColor
          font.family: root.uiFamily
          font.pixelSize: root.searchPx
          leftPadding: 0
          rightPadding: 0
          topPadding: 0
          bottomPadding: 0
          background: Item {}
          activeFocusOnTab: true
          Accessible.role: Accessible.EditableText
          Accessible.name: "Search things"
          Accessible.description: "Type part of a title; characters may be spread out (fuzzy), or a group name"
          Accessible.editable: true
          Accessible.focusable: true
          Accessible.focused: activeFocus
          onActiveFocusChanged: if (activeFocus) root.disarmPointer()
          onTextChanged: root.scheduleRankedItems()

          Keys.priority: Keys.BeforeItem
          Keys.onPressed: function(event) {
            var ctrl = (event.modifiers & Qt.ControlModifier) !== 0
            if (root.answerMode) {
              var step = Math.round(root.titlePx * 3)
              if (event.key === Qt.Key_Down) { event.accepted = true; root.scrollAnswer(step); return }
              if (event.key === Qt.Key_Up) { event.accepted = true; root.scrollAnswer(-step); return }
              if (event.key === Qt.Key_PageDown) { event.accepted = true; root.scrollAnswer(answerFlick.height * 0.9); return }
              if (event.key === Qt.Key_PageUp) { event.accepted = true; root.scrollAnswer(-answerFlick.height * 0.9); return }
              if (ctrl && event.key === Qt.Key_O) {
                event.accepted = true
                if (root.everythingService && root.askState.sessionId) {
                  root.everythingService.openAskInOpencode()
                  root.close()
                }
                return
              }
              if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                event.accepted = true
                if (!root.askRunning) root.copyAnswer()
                return
              }
            }
            if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N)) {
              event.accepted = true
              root.moveSelection(1, true)
            } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P)) {
              event.accepted = true
              root.moveSelection(-1, true)
            } else if (event.key === Qt.Key_PageDown) {
              event.accepted = true
              root.moveSelection(root.pageDelta(1), true)
            } else if (event.key === Qt.Key_PageUp) {
              event.accepted = true
              root.moveSelection(root.pageDelta(-1), true)
            } else if (event.key === Qt.Key_Tab && (event.modifiers & Qt.ShiftModifier) === 0) {
              // Browse mode keeps the row you were on.
              event.accepted = true
              root.focusList(resultList.currentIndex < 0 ? 0 : resultList.currentIndex)
            } else if (event.key === Qt.Key_Escape) {
              event.accepted = true
              root.handleEscape()
            } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
              // Launcher behaviour: Enter opens the highlighted result
              // straight from the search box.
              event.accepted = true
              if (resultList.currentIndex < 0 && resultList.count > 0)
                root.setCurrentIndex(0, false)
              root.activateCurrent()
            }
          }
        }

        Row {
          id: clearHint
          anchors.right: parent.right
          anchors.rightMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          spacing: Style.space(6)
          visible: searchField.text.length > 0

          KeyCap {
            anchors.verticalCenter: parent.verticalCenter
            label: "esc"
            ink: root.foreground
            pixelSize: root.hintPx
          }
          Text {
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: "clear"
            color: root.faintForeground
            font.family: root.uiFamily
            font.pixelSize: root.hintPx
          }
        }
      }

      // --- header: group checklist ---------------------------------------

      Item {
        id: groupHeader
        objectName: "everything-groups-header"
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: root.searchHeight
        visible: root.popupMode === root.groupsMode

        Text {
          id: groupHeaderGlyph
          anchors.left: parent.left
          anchors.leftMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          width: root.iconBox
          horizontalAlignment: Text.AlignHCenter
          textFormat: Text.PlainText
          text: ""
          color: root.selectedText
          font.family: Style.font.family
          font.pixelSize: root.iconPx
        }

        Text {
          anchors.left: groupHeaderGlyph.right
          anchors.leftMargin: Style.space(10)
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          text: "Show in Everything"
          color: root.foreground
          font.family: root.uiFamily
          font.pixelSize: root.titlePx
          font.weight: Font.Medium
        }

        Text {
          anchors.right: parent.right
          anchors.rightMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          text: root.shownGroupCount + " of " + root.checklistEntries.length
          color: root.faintForeground
          font.family: root.uiFamily
          font.pixelSize: root.metaPx
        }
      }

      Rectangle {
        id: headerDivider
        objectName: "everything-header-divider"
        anchors.top: parent.top
        anchors.topMargin: root.searchHeight
        anchors.left: parent.left
        anchors.leftMargin: -popup.padding
        anchors.right: parent.right
        anchors.rightMargin: -popup.padding
        height: Style.spacing.hairline
        color: root.hairlineColor
      }

      // --- footer: key hints + count -------------------------------------

      Item {
        id: footer
        objectName: "everything-footer"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        implicitHeight: Style.space(22)
        height: implicitHeight
        // Centre the hints between the hairline and the card's bottom edge.
        readonly property real centerOffset: popup.padding / 2

        Rectangle {
          anchors.top: parent.top
          anchors.left: parent.left
          anchors.leftMargin: -popup.padding
          anchors.right: parent.right
          anchors.rightMargin: -popup.padding
          height: Style.spacing.hairline
          color: root.hairlineColor
        }

        Row {
          id: hintRow
          anchors.left: parent.left
          anchors.leftMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          anchors.verticalCenterOffset: footer.centerOffset
          spacing: Style.space(14)

          Repeater {
            model: root.footerHints

            delegate: Row {
              id: hint
              required property var modelData
              spacing: Style.space(6)

              KeyCap {
                anchors.verticalCenter: parent.verticalCenter
                label: hint.modelData.key
                ink: root.foreground
                pixelSize: root.hintPx
              }
              Text {
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: hint.modelData.label
                color: root.faintForeground
                font.family: root.uiFamily
                font.pixelSize: root.hintPx
              }
            }
          }
        }

        Text {
          anchors.right: parent.right
          anchors.rightMargin: root.rowInset
          anchors.verticalCenter: parent.verticalCenter
          anchors.verticalCenterOffset: footer.centerOffset
          visible: root.popupMode === root.resultsMode
          textFormat: Text.PlainText
          text: root.rankedItems.length === 0 && root.everythingService
              && root.everythingService.scanning
            ? "Looking…"
            : root.rankedItems.length + (root.rankedItems.length === 1 ? " result" : " results")
          color: root.faintForeground
          font.family: root.uiFamily
          font.pixelSize: root.hintPx
        }
      }

      // --- empty state ---------------------------------------------------

      Column {
        id: emptyState
        objectName: "everything-empty"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: resultList.verticalCenter
        width: Math.min(parent.width - root.rowInset * 2, Style.space(460))
        spacing: Style.space(8)
        visible: root.popupMode === root.resultsMode && root.rankedItems.length === 0
          && !root.answerMode
        Accessible.role: Accessible.StaticText
        Accessible.name: emptyTitle.text

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          textFormat: Text.PlainText
          text: searchField.text.length > 0 ? "" : ""
          color: root.faintForeground
          font.family: Style.font.family
          font.pixelSize: Style.font.displayLarge
        }

        Text {
          id: emptyTitle
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          textFormat: Text.PlainText
          text: searchField.text.length > 0
            ? "No matches for “" + searchField.text + "”"
            : "Nothing open yet"
          color: root.dimForeground
          font.family: root.uiFamily
          font.pixelSize: root.titlePx
          elide: Text.ElideMiddle
        }

        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          textFormat: Text.PlainText
          text: searchField.text.length > 0
            ? "Try fewer letters, or a group name like “tabs”"
            : "Type to search apps and the Omarchy menu"
          color: root.faintForeground
          font.family: root.uiFamily
          font.pixelSize: root.metaPx
          wrapMode: Text.WordWrap
        }
      }

      // --- results -------------------------------------------------------

      ListView {
        id: resultList
        objectName: "everything-results"
        anchors.top: headerDivider.bottom
        anchors.topMargin: Style.space(6)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        anchors.bottomMargin: Style.space(6)
        visible: root.popupMode === root.resultsMode && !root.answerMode
        enabled: visible
        clip: true
        model: root.rankedItems
        currentIndex: -1
        activeFocusOnTab: count > 0
        boundsBehavior: Flickable.StopAtBounds
        keyNavigationEnabled: false
        highlightMoveDuration: 80
        Accessible.role: Accessible.List
        Accessible.name: "Ranked thing list"
        Accessible.description: "Use Up and Down to select; Left and Right collapse or expand the current group"
        Accessible.focusable: count > 0
        Accessible.focused: activeFocus

        section.property: "kind"
        section.criteria: ViewSection.FullString
        section.delegate: Item {
          id: sectionHeading
          required property string section
          objectName: "everything-section-" + section
          width: ListView.view.width
          height: root.sectionHeight
          readonly property bool collapsed: root.groupCollapsed(section)
          readonly property bool highlighted: collapsed && root.resultsModeOpen
            && root.currentGroupKind() === section

          Accessible.role: Accessible.Heading
          Accessible.name: Model.kindSectionLabel(sectionHeading.section)
            + (collapsed ? ", collapsed" : ", expanded")
          Accessible.focusable: collapsed
          Accessible.focused: highlighted
          Accessible.onPressAction: root.toggleGroupFromHeading(section)

          Rectangle {
            anchors.fill: parent
            anchors.topMargin: Style.space(4)
            radius: Style.cornerRadius
            color: sectionHeading.highlighted ? root.selectedFill : "transparent"
          }

          Text {
            id: sectionLabel
            anchors.left: parent.left
            anchors.leftMargin: root.rowInset + Style.space(2)
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Style.space(6)
            textFormat: Text.PlainText
            text: Model.kindSectionLabel(sectionHeading.section).toUpperCase()
            color: sectionHeading.highlighted ? root.selectedText : root.faintForeground
            font.family: root.uiFamily
            font.pixelSize: root.sectionPx
            font.weight: Font.Medium
            font.letterSpacing: Style.spaceReal(0.9)
          }

          Text {
            anchors.left: sectionLabel.right
            anchors.leftMargin: Style.space(8)
            anchors.baseline: sectionLabel.baseline
            textFormat: Text.PlainText
            // A single instant answer needs no count.
            visible: ["calc", "convert", "time", "ask", "timer"].indexOf(sectionHeading.section) < 0
            text: root.kindCount(sectionHeading.section)
            color: root.faintForeground
            font.family: root.uiFamily
            font.features: { "tnum": 1 }
            font.pixelSize: root.sectionPx
          }

          Text {
            id: sectionDisclosure
            anchors.right: parent.right
            anchors.rightMargin: root.rowInset
            anchors.verticalCenter: sectionLabel.verticalCenter
            visible: sectionHeading.collapsed || headingMouse.containsMouse
            textFormat: Text.PlainText
            text: sectionHeading.collapsed ? "" : ""
            color: sectionHeading.highlighted ? root.selectedText : root.faintForeground
            font.family: Style.font.family
            font.pixelSize: Math.round(root.sectionPx * 0.85)
          }

          MouseArea {
            id: headingMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.toggleGroupFromHeading(sectionHeading.section)
          }
        }

        Keys.priority: Keys.BeforeItem
        Keys.onPressed: function(event) { root.handleSupplementalListKey(event) }
        onCountChanged: {
          if (!root.resultUpdateInProgress) {
            if (count <= 0) root.setCurrentIndex(-1, false)
            else if (currentIndex < 0) root.setCurrentIndex(0, false)
            else if (currentIndex >= count) root.setCurrentIndex(count - 1, false)
          }
        }

        ScrollBar.vertical: ScrollBar {
          objectName: "everything-scrollbar"
          policy: ScrollBar.AsNeeded
          leftPadding: Style.space(4)
          rightPadding: 0
          palette.mid: Commons.Color.accent
          palette.dark: Commons.Color.accent
        }

        delegate: Item {
          id: row
          required property var modelData
          required property int index
          width: resultList.width
          height: groupCollapsed ? 0 : root.rowHeight
          visible: !groupCollapsed
          enabled: !groupCollapsed
          readonly property bool groupCollapsed: root.groupCollapsed(modelData.kind)
          readonly property bool isAnswer: Answers.isAnswerKind(modelData.kind)
          readonly property bool emphasis: modelData.emphasis === true
          readonly property string thumbnail: String(modelData.thumbnail || "")
          // The highlight is always shown: Enter in the search line opens it.
          readonly property bool highlighted: !groupCollapsed && root.resultsModeOpen
            && resultList.currentIndex === index
          readonly property color iconColor: highlighted ? root.selectedText : root.dimForeground
          readonly property string crumb: root.breadcrumb(modelData)
          readonly property string metadata: Model.metadataForItem(
            modelData, root.itemRelations)
          readonly property var windowEntry: root.resolveWindowDesktopEntry(modelData)
          readonly property string windowAppGlyph: Model.windowAppGlyph(modelData, windowEntry)
          readonly property string windowIconSource: windowAppGlyph.length === 0
            ? root.windowIconSource(modelData, windowEntry) : ""

          Accessible.role: Accessible.ListItem
          Accessible.name: String(modelData.title || "Untitled")
          Accessible.description: [String(modelData.provider || ""), Model.kindLabel(modelData.kind)]
            .concat(Array.isArray(modelData.badges) ? modelData.badges : [])
            .concat(modelData.active === true ? ["Current thing"] : [])
            .concat(crumb ? [crumb] : [])
            .filter(function(value) { return value.length > 0 }).join(", ")
          Accessible.selectable: true
          Accessible.selected: highlighted
          Accessible.focusable: true
          Accessible.focused: highlighted
          Accessible.onPressAction: root.activateAt(index)

          Rectangle {
            anchors.fill: parent
            radius: Style.cornerRadius
            color: row.highlighted ? root.selectedFill : "transparent"
          }

          Rectangle {
            objectName: "everything-row-marker-" + row.index
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(3)
            height: Math.round(parent.height * 0.46)
            radius: width / 2
            color: root.selectedText
            visible: row.highlighted
          }

          Item {
            id: thingIcon
            anchors.left: parent.left
            anchors.leftMargin: root.rowInset
            width: root.iconBox
            height: parent.height
            Accessible.role: Accessible.Graphic
            Accessible.name: Model.kindLabel(row.modelData.kind) + " icon"

            Image {
              objectName: "everything-thumbnail-" + row.index
              anchors.centerIn: parent
              width: parent.width
              height: Math.round(root.rowHeight * 0.72)
              visible: row.thumbnail.length > 0 && status === Image.Ready
              source: row.thumbnail
              fillMode: Image.PreserveAspectCrop
              asynchronous: true
              smooth: true
              sourceSize.width: width * Screen.devicePixelRatio
              sourceSize.height: height * Screen.devicePixelRatio
            }

            Image {
              id: applicationIcon
              objectName: "everything-window-icon-" + row.index
              anchors.centerIn: parent
              width: Math.min(parent.width, root.iconPx + Style.space(2))
              height: width
              visible: row.windowAppGlyph.length === 0
                && row.windowIconSource.length > 0 && status !== Image.Error
              source: row.windowIconSource
              fillMode: Image.PreserveAspectFit
              asynchronous: true
              smooth: true
              mipmap: true
              // Decode at physical pixels; a logical-size decode is blurry on HiDPI.
              sourceSize.width: width * Screen.devicePixelRatio
              sourceSize.height: height * Screen.devicePixelRatio
              // App launcher icons keep their own colors; window icons are tinted.
              layer.enabled: visible && row.modelData.kind !== "app"
              layer.effect: MultiEffect {
                colorization: 1.0
                colorizationColor: row.iconColor
              }
            }

            OpticalGlyph {
              id: mappedWindowGlyph
              objectName: "everything-window-glyph-" + row.index
              anchors.fill: parent
              visible: row.windowAppGlyph.length > 0
              text: row.windowAppGlyph
              color: row.iconColor
              fontFamily: Style.font.family
              fontSize: root.iconPx
            }

            Text {
              id: fallbackThingIcon
              anchors.fill: parent
              visible: row.windowAppGlyph.length === 0 && !applicationIcon.visible
                && row.thumbnail.length === 0
              textFormat: Text.PlainText
              text: Model.kindIcon(row.modelData.kind)
              color: row.iconColor
              font.family: Style.font.family
              font.pixelSize: root.iconPx
              horizontalAlignment: Text.AlignHCenter
              verticalAlignment: Text.AlignVCenter
            }
          }

          Text {
            id: returnHint
            anchors.right: parent.right
            anchors.rightMargin: root.rowInset
            anchors.baseline: titleLabel.baseline
            width: Style.space(16)
            horizontalAlignment: Text.AlignRight
            textFormat: Text.PlainText
            text: "↵"
            color: root.selectedText
            opacity: row.highlighted && !(row.isAnswer && !row.modelData.local) ? 0.9 : 0
            font.family: Style.font.family
            font.pixelSize: root.metaPx
          }

          TextMetrics {
            id: metadataMetrics
            text: row.metadata
            font.family: root.uiFamily
            font.pixelSize: root.metaPx
          }

          Text {
            id: metadataLabel
            objectName: "everything-metadata-" + row.index
            anchors.right: returnHint.left
            anchors.rightMargin: Style.space(10)
            // Share the title's baseline so mixed sizes read as one line.
            anchors.baseline: titleLabel.baseline
            visible: text.length > 0
            width: visible ? Math.min(metadataMetrics.advanceWidth + 1, resultList.width * 0.36) : 0
            textFormat: Text.PlainText
            text: row.metadata
            color: row.highlighted ? root.dimForeground : root.faintForeground
            font.family: root.uiFamily
            font.pixelSize: root.metaPx
            horizontalAlignment: Text.AlignRight
            elide: Text.ElideMiddle
            Accessible.role: Accessible.StaticText
            Accessible.name: text
          }

          Text {
            id: titleLabel
            objectName: "everything-title-" + row.index
            anchors.left: thingIcon.right
            anchors.leftMargin: Style.space(10)
            anchors.right: metadataLabel.left
            anchors.rightMargin: metadataLabel.visible ? Style.space(16) : 0
            anchors.verticalCenter: parent.verticalCenter
            // Escaped by Model.highlightMarkup; only its own <b>/<font> tags remain.
            textFormat: Text.StyledText
            text: Model.highlightMarkup(row.modelData.title || "Untitled",
              row.modelData.highlightQuery !== undefined ? String(row.modelData.highlightQuery)
                : (row.isAnswer ? "" : searchField.text), root.matchColor)
            color: row.highlighted ? root.selectedText : root.foreground
            font.family: root.uiFamily
            font.pixelSize: row.emphasis ? Math.round(root.titlePx * 1.15) : root.titlePx
            font.weight: row.emphasis ? Font.Medium : Font.Normal
            font.features: row.emphasis ? { "tnum": 1 } : {}
            maximumLineCount: 1
            elide: Text.ElideRight
          }

          MouseArea {
            id: rowMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onEntered: root.selectFromPointer(row.index, row, {
              x: rowMouse.mouseX,
              y: rowMouse.mouseY
            })
            onPositionChanged: function(mouse) {
              root.selectFromPointer(row.index, row, mouse)
            }
            onClicked: {
              root.setCurrentIndex(row.index, false)
              root.pointerCursorActive = true
              root.activateAt(row.index)
            }
          }
        }
      }

      // Scroll scrims: fade rows into the card at whichever edge hides more.
      Rectangle {
        anchors.top: resultList.top
        anchors.left: resultList.left
        anchors.right: resultList.right
        anchors.rightMargin: Style.space(8)
        height: Math.min(Style.space(24), resultList.height / 2)
        visible: resultList.visible && opacity > 0
        opacity: resultList.contentHeight > resultList.height
          ? Math.max(0, Math.min(1, (resultList.contentY - resultList.originY) / height))
          : 0
        gradient: Gradient {
          GradientStop { position: 0; color: popup.cardColor }
          GradientStop { position: 1; color: Util.alpha(popup.cardColor, 0) }
        }
      }

      Rectangle {
        anchors.bottom: resultList.bottom
        anchors.left: resultList.left
        anchors.right: resultList.right
        anchors.rightMargin: Style.space(8)
        height: Math.min(Style.space(24), resultList.height / 2)
        visible: resultList.visible && opacity > 0
        opacity: resultList.contentHeight > resultList.height
          ? Math.max(0, Math.min(1, (resultList.originY + resultList.contentHeight
              - resultList.height - resultList.contentY) / height))
          : 0
        gradient: Gradient {
          GradientStop { position: 0; color: Util.alpha(popup.cardColor, 0) }
          GradientStop { position: 1; color: popup.cardColor }
        }
      }

      // --- ask: answer view ----------------------------------------------

      Item {
        id: answerView
        objectName: "everything-answer"
        anchors.top: headerDivider.bottom
        anchors.topMargin: Style.space(10)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        anchors.bottomMargin: Style.space(8)
        visible: root.answerMode

        Item {
          id: answerHeader
          anchors.top: parent.top
          anchors.left: parent.left
          anchors.right: parent.right
          height: Math.round(root.metaPx * 1.8)

          Text {
            id: answerGlyph
            anchors.left: parent.left
            anchors.leftMargin: root.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: root.iconBox
            horizontalAlignment: Text.AlignHCenter
            textFormat: Text.PlainText
            text: Model.kindIcon("ask")
            color: root.selectedText
            font.family: Style.font.family
            font.pixelSize: root.iconPx
          }

          Text {
            anchors.left: answerGlyph.right
            anchors.leftMargin: Style.space(10)
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: "opencode · " + root.askLabel
            color: root.dimForeground
            font.family: root.uiFamily
            font.pixelSize: root.metaPx
            font.weight: Font.Medium
          }

          Text {
            anchors.right: parent.right
            anchors.rightMargin: root.rowInset
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: {
              var st = root.askState
              var started = Number(st.startedAt || 0)
              if (st.status === "running") {
                var secs = Math.max(0, Math.round((root.askClock - started) / 1000))
                return (st.tool ? "using " + st.tool : "thinking") + "… " + secs + "s"
              }
              var took = Math.max(0, (Number(st.finishedAt || 0) - started) / 1000)
              if (st.status === "error") return "failed"
              if (st.status === "cancelled") return "cancelled"
              return "answered in " + took.toFixed(1) + "s"
                + (st.cost > 0 ? " · $" + Number(st.cost).toFixed(4) : "")
            }
            color: root.faintForeground
            font.family: root.uiFamily
            font.pixelSize: root.hintPx
            font.features: { "tnum": 1 }
          }
        }

        Flickable {
          id: answerFlick
          anchors.top: answerHeader.bottom
          anchors.topMargin: Style.space(10)
          anchors.left: parent.left
          anchors.leftMargin: root.rowInset + root.iconBox + Style.space(10)
          anchors.right: parent.right
          anchors.rightMargin: root.rowInset
          anchors.bottom: parent.bottom
          clip: true
          contentWidth: width
          contentHeight: answerText.implicitHeight
          boundsBehavior: Flickable.StopAtBounds
          ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

          Text {
            id: answerText
            objectName: "everything-answer-text"
            width: answerFlick.width - Style.space(8)
            // Untrusted model output: Answers.safeMarkdown strips images and
            // escapes raw HTML before it reaches the Markdown sink.
            textFormat: root.askState.answer ? Text.MarkdownText : Text.PlainText
            text: root.askState.answer
              ? Answers.safeMarkdown(root.askState.answer)
              : (root.askState.status === "error" ? String(root.askState.error || "Something went wrong")
                : (root.askState.status === "running" ? "Thinking…" : "No answer"))
            color: root.askState.answer ? root.foreground
              : (root.askState.status === "error" ? Commons.Color.urgent : root.faintForeground)
            wrapMode: Text.Wrap
            font.family: root.uiFamily
            font.pixelSize: Math.round(root.titlePx * 0.9)
            lineHeight: 1.25
            onLinkActivated: function(link) {
              if (/^https?:\/\//i.test(String(link))) Qt.openUrlExternally(link)
            }
          }
        }
      }

      // --- group checklist -----------------------------------------------

      ListView {
        id: groupList
        objectName: "everything-groups"
        anchors.top: headerDivider.bottom
        anchors.topMargin: Style.space(6)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        anchors.bottomMargin: Style.space(6)
        visible: root.popupMode === root.groupsMode
        enabled: visible
        clip: true
        model: root.checklistEntries
        currentIndex: 0
        activeFocusOnTab: true
        boundsBehavior: Flickable.StopAtBounds
        keyNavigationEnabled: false
        highlightMoveDuration: 80
        Accessible.role: Accessible.List
        Accessible.name: "Visible thing groups"
        Accessible.description: "Checked groups appear in Everything search results"
        Accessible.focusable: true
        Accessible.focused: activeFocus

        ScrollBar.vertical: ScrollBar {
          policy: ScrollBar.AsNeeded
          leftPadding: Style.space(4)
          rightPadding: 0
          palette.mid: Commons.Color.accent
          palette.dark: Commons.Color.accent
        }

        delegate: Item {
          id: groupRow
          required property var modelData
          required property int index
          objectName: "everything-group-" + modelData.kind
          width: groupList.width
          height: root.groupChecklistRowHeight
          readonly property bool checked: !root.kindDisabled(modelData.kind)
          readonly property bool highlighted: groupList.currentIndex === index && root.opened

          function activate() {
            root.setGroupChecklistIndex(index)
            groupList.forceActiveFocus()
            root.toggleChecklistEntry(modelData)
          }

          Accessible.role: Accessible.CheckBox
          Accessible.name: modelData.sectionLabel
          Accessible.description: checked ? "Shown in Everything search results" : "Hidden from Everything search results"
          Accessible.checkable: true
          Accessible.checked: checked
          Accessible.focusable: true
          Accessible.focused: highlighted
          Accessible.onPressAction: groupRow.activate()
          Accessible.onToggleAction: groupRow.activate()

          Rectangle {
            anchors.fill: parent
            radius: Style.cornerRadius
            color: groupRow.highlighted ? root.selectedFill : "transparent"
          }

          Rectangle {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(3)
            height: Math.round(parent.height * 0.46)
            radius: width / 2
            color: root.selectedText
            visible: groupRow.highlighted
          }

          Text {
            id: groupIcon
            anchors.left: parent.left
            anchors.leftMargin: root.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: root.iconBox
            textFormat: Text.PlainText
            text: groupRow.modelData.icon
            color: groupRow.highlighted ? root.selectedText
              : (groupRow.checked ? root.dimForeground : root.faintForeground)
            font.family: Style.font.family
            font.pixelSize: root.iconPx
            horizontalAlignment: Text.AlignHCenter
          }

          Text {
            anchors.left: groupIcon.right
            anchors.leftMargin: Style.space(10)
            anchors.right: groupCheckmark.left
            anchors.rightMargin: Style.space(10)
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: groupRow.modelData.sectionLabel
            color: groupRow.highlighted ? root.selectedText
              : (groupRow.checked ? root.foreground : root.faintForeground)
            font.family: root.uiFamily
            font.pixelSize: root.titlePx
            elide: Text.ElideRight
          }

          Text {
            id: groupCheckmark
            anchors.right: parent.right
            anchors.rightMargin: root.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: root.iconBox
            textFormat: Text.PlainText
            text: groupRow.checked ? "" : ""
            color: groupRow.checked ? root.selectedText : root.faintForeground
            font.family: Style.font.family
            font.pixelSize: root.iconPx
            horizontalAlignment: Text.AlignHCenter
          }

          MouseArea {
            objectName: "everything-group-pointer-" + groupRow.modelData.kind
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: groupRow.activate()
          }
        }
      }

    }
  }
}
}
