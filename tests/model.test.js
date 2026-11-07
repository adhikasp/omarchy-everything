const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

const root = path.resolve(__dirname, "..")
const source = fs.readFileSync(path.join(root, "EverythingModel.js"), "utf8")
  .replace(/^\.pragma library\s*/m, "")
const sandbox = {}
vm.createContext(sandbox)
vm.runInContext(source, sandbox, { filename: "EverythingModel.js" })
// InstantAnswers.js imports the model as `Model`; share one sandbox.
sandbox.Model = sandbox
vm.runInContext(fs.readFileSync(path.join(root, "InstantAnswers.js"), "utf8")
  .replace(/^\.pragma library\s*/m, "").replace(/^\.import .*$/m, ""),
  sandbox, { filename: "InstantAnswers.js" })

const item = (id, title, context, extra = {}) => ({
  id,
  uiUuid: `test-uuid:${id}`,
  kind: "window",
  provider: "Test",
  title,
  context,
  searchTerms: [],
  badges: [],
  active: false,
  recency: 0,
  ...extra,
})

{
  const catalog = sandbox.supportedGroupCatalog()
  const expectedKinds = [
    "calc", "convert", "time", "timer", "ask", "clipboard", "window", "browser-tab", "app-tab", "terminal-tab", "terminal-pane",
    "herdr-agent", "herdr-workspace", "herdr-tab", "herdr-pane", "herdr-session",
    "tmux-session", "tmux-window", "tmux-pane", "neovim-buffer", "app", "omarchy-menu",
  ]
  const expectedSections = [
    "Calculator", "Conversions", "Time zones", "Timers", "Ask opencode", "Clipboard history", "Windows", "Browser tabs", "Application tabs", "Terminal tabs", "Terminal panes",
    "Herdr agents", "Herdr workspaces", "Herdr tabs", "Herdr panes", "Herdr sessions",
    "tmux sessions", "tmux windows", "tmux panes", "Buffers", "Apps", "Omarchy menu",
  ]
  assert.deepEqual(Array.from(catalog, group => group.kind), expectedKinds,
    "the catalog contains all supported groups in display order")
  assert.deepEqual(Array.from(catalog, group => group.sectionLabel), expectedSections,
    "the checklist and result headings share their labels")
  for (let index = 0; index < catalog.length; index++) {
    assert.equal(catalog[index].order, index, `${catalog[index].kind} has a stable order`)
    assert.equal(sandbox.kindOrder(catalog[index].kind), index)
    assert.equal(sandbox.kindLabel(catalog[index].kind), catalog[index].label)
    assert.equal(sandbox.kindSectionLabel(catalog[index].kind), catalog[index].sectionLabel)
  }
  assert.equal(sandbox.kindOrder("__proto__"), 1000,
    "unknown object-prototype names remain unsupported kinds")

  const normalized = value => Array.from(sandbox.normalizeDisabledKinds(value))
  assert.deepEqual(normalized(undefined), [], "missing settings enable every group")
  assert.deepEqual(normalized(null), [])
  assert.deepEqual(normalized("window"), [], "a scalar string is not a valid string array")
  assert.deepEqual(normalized({ window: true }), [], "objects are not valid string arrays")
  assert.deepEqual(normalized({ 0: "window", length: Infinity }), [],
    "malformed array-like lengths are ignored")
  assert.deepEqual(normalized([
    "tmux-pane", "unknown", 12, null, "browser-tab", "tmux-pane",
    " Browser-tab", "WINDOW",
  ]), ["browser-tab", "tmux-pane"],
  "normalization ignores malformed and unknown entries, deduplicates, and restores catalog order")
  assert.deepEqual(normalized({ 0: "neovim-buffer", 1: "window", length: 2 }),
    ["window", "neovim-buffer"], "Qt-style string lists are accepted")
}

{
  const rows = [
    item("window", "Window", "", { kind: "window" }),
    item("browser", "Browser", "", { kind: "browser-tab" }),
    item("buffer", "Buffer", "", { kind: "neovim-buffer" }),
  ]
  assert.deepEqual(Array.from(sandbox.rank(rows, "", {}), row => row.id),
    ["window", "browser", "buffer"], "omitting disabled kinds leaves every group enabled")
  assert.deepEqual(Array.from(sandbox.rank(rows, "", {}, ["browser-tab"]), row => row.id),
    ["window", "buffer"], "disabled groups are excluded before ranking")
  assert.deepEqual(Array.from(sandbox.rank(rows, "browser", {}, ["browser-tab"]), row => row.id),
    [], "search cannot restore a disabled group")
  assert.deepEqual(Array.from(sandbox.rank(rows, "", {}, ["unknown"]), row => row.id),
    ["window", "browser", "buffer"], "unknown disabled kinds do not hide results")
  const allKinds = Array.from(sandbox.supportedGroupCatalog(), group => group.kind)
  assert.deepEqual(Array.from(sandbox.rank(rows, "", {}, allKinds), row => row.id), [],
    "disabling every supported group intentionally yields no results")
}

{
  const rows = [
    item("window", "Window", "", { kind: "window" }),
    item("browser", "Browser", "", { kind: "browser-tab" }),
    item("agent", "Agent", "", { kind: "herdr-agent" }),
  ]
  const ids = lead => Array.from(sandbox.rank(rows, "", {}, [], lead), row => row.id)
  assert.equal(sandbox.contextLeadKind("google-chrome", rows), "browser-tab")
  assert.equal(sandbox.contextLeadKind("com.mitchellh.ghostty", rows), "herdr-agent")
  assert.deepEqual(Array.from(sandbox.rank(rows.slice(0, 2), "", {}, [], "herdr-agent"), row => row.id),
    ["window", "browser"], "a terminal without Herdr agents falls back to the default order")
  assert.equal(sandbox.contextLeadKind("org.gnome.Nautilus", rows), "")
  assert.deepEqual(ids(""), ["window", "browser", "agent"], "no context keeps the default order")
  assert.deepEqual(ids("herdr-agent"), ["agent", "window", "browser"], "terminal focus leads with agents")
  assert.deepEqual(ids("browser-tab"), ["browser", "window", "agent"], "Chrome focus leads with tabs")
}

{
  const rows = [
    item("context", "Unrelated", "Project Alpha", { active: true, recency: 4000 }),
    item("title", "Alpha notes", "Old", { recency: -4000 }),
  ]
  assert.equal(sandbox.rank(rows, "alpha", {}).length, 1,
    "context-only matches are excluded")
  assert.equal(sandbox.rank(rows, "alpha", {})[0].id, "title", "title match wins before active/recency")
}

{
  const rows = [
    item("recent", "same", "same", { recency: 20 }),
    item("active", "same", "same", { active: true, recency: -20 }),
  ]
  assert.equal(sandbox.rank(rows, "same", {})[0].id, "active", "active thing wins before recency")
}

{
  const rows = [
    item("provider", "Unrelated", "Elsewhere", {
      provider: "Alpha", active: true, recency: 4000,
    }),
    item("context", "Unrelated", "Alpha project", { recency: -4000 }),
    item("badge", "Unrelated", "Elsewhere", { badges: ["Alpha"] }),
    item("term", "Unrelated", "Elsewhere", { searchTerms: ["Alpha"] }),
  ]
  assert.equal(sandbox.rank(rows, "alpha", {}).length, 0,
    "provider, context, badges, and hidden terms are not searchable")
  assert.equal(
    sandbox.rank([item("old", "A", ""), item("new", "B", "", { recency: 10 })], "", {})[0].id,
    "new",
    "recency breaks otherwise equal unfiltered rows",
  )
}

{
  const rows = [
    item("window", "Unrelated", "", { kind: "window" }),
    item("buffer", "Unrelated", "", { kind: "neovim-buffer" }),
  ]
  assert.deepEqual(Array.from(sandbox.rank(rows, "windows", {}), row => row.id), ["window"],
    "the displayed Windows group name is searchable")
  assert.deepEqual(Array.from(sandbox.rank(rows, "buffers", {}), row => row.id), ["buffer"],
    "the displayed Buffers group name is searchable")
}

{
  const rows = [
    item("pane", "Pane", "", { kind: "herdr-pane" }),
    item("session", "Session", "", { kind: "herdr-session" }),
    item("agent", "Agent", "", { kind: "herdr-agent" }),
    item("tab", "Tab", "", { kind: "herdr-tab" }),
    item("workspace", "Workspace", "", { kind: "herdr-workspace" }),
    item("tmux-pane", "Pane", "", { kind: "tmux-pane" }),
    item("tmux-session", "Session", "", { kind: "tmux-session" }),
    item("tmux-window", "Window", "", { kind: "tmux-window" }),
  ]
  assert.deepEqual(
    Array.from(sandbox.rank(rows, "", {}), row => row.id),
    [
      "agent", "workspace", "tab", "pane", "session",
      "tmux-session", "tmux-window", "tmux-pane",
    ],
    "Herdr groups precede tmux, with agents first and Herdr sessions last",
  )
}

{
  const rows = [item("one", "Alpha Beta", ""), item("two", "Alpha", "Gamma")]
  assert.deepEqual(Array.from(sandbox.rank(rows, "alpha be", {}), row => row.id), ["one"],
    "a contiguous multi-word title substring matches")
  assert.deepEqual(Array.from(sandbox.rank(rows, "alp bet", {}), row => row.id), ["one"],
    "separate query terms each match fuzzily, in any order")
  assert.deepEqual(Array.from(sandbox.rank(rows, "ab", {}), row => row.id), ["one"],
    "non-contiguous character subsequences match (fuzzy)")
  assert.deepEqual(Array.from(sandbox.rank(rows, "zz", {}), row => row.id), [],
    "characters that are not all present do not match")
  assert.deepEqual(Array.from(sandbox.rank(rows, "PHA B", {}), row => row.id), ["one"],
    "contiguous substring matching is case-insensitive")
  assert.deepEqual(Array.from(sandbox.rank([
    item("browser", "Unrelated", "", { kind: "browser-tab" }),
  ], "browser tab", {}), row => row.id), ["browser"],
  "a contiguous displayed-group substring matches")
  assert.deepEqual(Array.from(sandbox.rank([
    item("split", "Alpha", "", { kind: "window" }),
  ], "alpha windows", {}), row => row.id), [],
  "one query cannot be split between title and group")
}

{
  const rows = [
    item("pane-new", "Pane", "", { kind: "terminal-pane", recency: 20 }),
    item("window-old", "Window old", "", { kind: "window", recency: 1 }),
    item("tab", "Tab", "", { kind: "browser-tab", recency: 30 }),
    item("window-new", "Window new", "", { kind: "window", recency: 10 }),
    item("buffer", "Buffer", "", { kind: "neovim-buffer", recency: 40 }),
  ]
  assert.deepEqual(
    Array.from(sandbox.rank(rows, "", {}), row => row.id),
    ["window-new", "window-old", "tab", "pane-new", "buffer"],
    "results are grouped by kind and ranked within each group",
  )
  assert.equal(sandbox.kindSectionLabel("browser-tab"), "Browser tabs")
  assert.equal(sandbox.kindSectionLabel("neovim-buffer"), "Buffers")
  assert.equal(sandbox.sectionedRowTop(sandbox.rank(rows, "", {}), 0, 36, 24), 24)
  assert.equal(sandbox.sectionedRowTop(sandbox.rank(rows, "", {}), 2, 36, 24), 120)
}

{
  const rows = [
    item("window-one", "One", "", { kind: "window" }),
    item("window-two", "Two", "", { kind: "window" }),
    item("agent-one", "One", "", { kind: "herdr-agent" }),
    item("agent-two", "Two", "", { kind: "herdr-agent" }),
    item("tmux-one", "One", "", { kind: "tmux-pane" }),
    item("tmux-two", "Two", "", { kind: "tmux-pane" }),
  ]
  const collapsed = { "herdr-agent": true, "tmux-pane": true }
  assert.equal(sandbox.groupStartIndex(rows, 3), 2)
  assert.equal(sandbox.groupEndIndex(rows, 2), 3)
  assert.equal(sandbox.adjacentNavigableIndex(rows, 1, 1, collapsed), 2,
    "vertical movement enters a collapsed group at its heading")
  assert.equal(sandbox.adjacentNavigableIndex(rows, 3, 1, collapsed), 4,
    "vertical movement leaves a collapsed group without visiting hidden rows")
  assert.equal(sandbox.adjacentNavigableIndex(rows, 4, -1, collapsed), 2,
    "reverse movement targets the preceding collapsed heading")
  assert.equal(sandbox.boundaryNavigableIndex(rows, 1, collapsed), 4,
    "last targets the heading when the final group is collapsed")
  assert.equal(sandbox.sectionedRowTop(rows, 4, 36, 24, collapsed), 144,
    "collapsed rows contribute no height while every group retains its heading")
}

{
  const rows = [
    item("scratchpad", "Alpha scratchpad", "", {
      badges: ["Window", "Workspace special:scratch", "Scratchpad"],
      active: true,
      recency: 4000,
    }),
    item("workspace-10", "Alpha", "", {
      badges: ["Window", "Workspace 10"], active: true, recency: 4000,
    }),
    item("workspace-2", "Alpha notes", "", {
      badges: ["Window", "Workspace 2"], recency: 10,
    }),
    item("workspace-1", "Notes about alpha", "", {
      badges: ["Window", "Workspace 1"], recency: -4000,
    }),
  ]
  assert.deepEqual(
    Array.from(sandbox.rank(rows, "alpha", {}), row => row.id),
    ["workspace-1", "workspace-2", "workspace-10", "scratchpad"],
    "window metadata sorts naturally with Scratchpad after regular workspaces",
  )
}

{
  const rows = [
    item("parent-zulu", "Zulu", "", { kind: "herdr-tab" }),
    item("child-zulu", "Child Zulu", "Fallback", {
      kind: "herdr-pane", parentId: "parent-zulu", badges: ["Herdr pane"],
    }),
    item("parent-alpha", "Alpha", "", { kind: "herdr-tab" }),
    item("child-alpha", "Child Alpha", "Fallback", {
      kind: "herdr-pane", parentId: "parent-alpha", badges: ["Herdr pane"],
    }),
  ]
  assert.deepEqual(
    Array.from(sandbox.rank(rows, "child", {
      "parent-alpha": true,
      "parent-zulu": true,
    }, ["herdr-tab"]), row => row.id),
    ["child-alpha", "child-zulu"],
    "enabled children keep parent-derived ordering when their parent group is hidden and disabled",
  )
}

{
  assert.equal(sandbox.kindIcon("herdr-agent"), "󱚣",
    "Herdr agents use Omarchy's agent-usage robot glyph")
  assert.notEqual(sandbox.kindIcon("window"), sandbox.kindIcon("neovim-buffer"))
  const entries = [
    {
      id: "foot.desktop", name: "Foot", startupClass: "foot",
      execString: "foot", icon: "foot",
    },
    {
      id: "Slack Loadup.desktop", name: "Slack Loadup", startupClass: "",
      execString: "omarchy-launch-webapp https://app.slack.com/client/T0BD9A3HVQF",
      icon: "slack-loadup",
    },
  ]
  assert.equal(sandbox.matchDesktopEntry(["foot"], entries), entries[0],
    "Hyprland application classes resolve native desktop entries")
  assert.equal(sandbox.matchDesktopEntry([
    "chrome-app.slack.com__client_T0BD9A3HVQF-Default",
    "app.slack.com_/client/T0BD9A3HVQF",
  ], entries), entries[1], "Hyprland web-app identity resolves its own icon")
  assert.equal(sandbox.windowAppGlyph(
    item("foot-window", "Foot", "", { iconHints: ["foot"] }), entries[0]),
    "\ue795", "known Omarchy apps use their default-font glyph")
  assert.equal(sandbox.windowAppGlyph(item("ghostty-window", "Ghostty", "", {
    iconHints: ["com.mitchellh.ghostty"],
  }), null), "\ue795", "an exact Hyprland app identity can select a known glyph")
  assert.equal(sandbox.windowAppGlyph(item("slack-window", "Slack", "", {
    iconHints: [
      "chrome-app.slack.com__client_T0BD9A3HVQF-Default",
      "google-chrome",
    ],
  }), entries[1]), "", "web apps do not inherit their host browser's glyph")
  assert.equal(sandbox.windowAppGlyph(item("docker-window", "Docker", "", {
    iconHints: ["unrelated-class"],
  }), {
    id: "com.docker.desktop.desktop", name: "Docker Desktop",
    startupClass: "", execString: "docker-desktop", icon: "docker",
  }), "\uf21f", "entry-only identities select a glyph after desktop-entry resolution")
  assert.equal(sandbox.windowAppGlyph(item("raw-docker", "Docker", "", {
    iconHints: ["docker"],
  }), null), "", "entry-only identities never match an unverified raw class")
  assert.equal(sandbox.windowAppGlyph(item("not-window", "Foot", "", {
    kind: "terminal-pane", iconHints: ["foot"],
  }), entries[0]), "", "app glyph mapping is limited to managed-window rows")
  assert.equal(sandbox.vitalMetadata(
    item("window", "Window", "App · workspace 4", { badges: ["Window", "Workspace 4"] }), ""),
    "Workspace 4")
  assert.equal(sandbox.vitalMetadata(
    item("qt-window", "Window", "App · workspace 1", {
      badges: { 0: "Window", 1: "Workspace 1", length: 2 },
    }), ""), "Workspace 1", "Qt-style list metadata excludes the kind")
  assert.equal(sandbox.vitalMetadata(
    item("joined-window", "Window", "App · workspace 1", {
      badges: "Window,Workspace 1",
    }), ""), "Workspace 1", "comma-serialized badge metadata excludes the kind")
  assert.equal(sandbox.vitalMetadata(
    item("buffer", "File", "/tmp/project", {
      kind: "neovim-buffer", badges: ["Buffer", "Modified", "Visible"],
    }), "Container › /tmp/project"),
    "project", "buffer metadata is its directory leaf, not a full path, status, or ancestry")
  assert.equal(sandbox.vitalMetadata(
    item("trailing-buffer", "File", "/tmp/project/src/", {
      kind: "neovim-buffer", badges: ["Buffer"],
    }), ""), "src", "buffer metadata ignores trailing path separators")
  assert.equal(sandbox.vitalMetadata(
    item("root-buffer", "File", "/", {
      kind: "neovim-buffer", badges: ["Buffer"],
    }), ""), "/", "the filesystem root remains meaningful buffer metadata")
  assert.equal(sandbox.vitalMetadata(
    item("browser", "Site", "Browser · native tab", { kind: "browser-tab", provider: "Firefox", badges: ["Tab", "Title only"] }), ""),
    "Firefox")
  assert.equal(sandbox.vitalMetadata(
    item("app-tab", "Layered image", "Layered image · native tab", {
      kind: "app-tab", provider: "Pinta", badges: ["Tab", "Native"],
    }), ""), "Pinta", "application-tab metadata names its owning app")
  assert.equal(sandbox.vitalMetadata(
    item("pane", "Shell", "/src", { kind: "terminal-pane", badges: ["Pane"] }), "Project › /src"),
    "Project › /src")
  assert.equal(sandbox.vitalMetadata(
    item("tmux-window", "Editor", "tmux window @4", { kind: "tmux-window", provider: "tmux", badges: ["Window"] }),
    "dev › tmux window @4", "dev"), "dev", "tmux windows show their session name")
  assert.equal(sandbox.vitalMetadata(
    item("tmux-pane", "Shell", "/src/project", { kind: "tmux-pane", provider: "tmux", badges: ["Pane"] }),
    "dev › editor › /src/project", "editor"), "editor", "tmux panes show their window name")
  assert.equal(sandbox.vitalMetadata(
    item("orphaned-tmux-pane", "Shell", "/src/project", {
      kind: "tmux-pane", provider: "tmux", badges: ["Pane"],
    }), "dev › editor › /src/project", ""), "tmux",
  "tmux child metadata never falls back to a native id or cwd chain")
  assert.equal(sandbox.vitalMetadata(
    item("surface", "Shell", "Ghostty surface", { kind: "terminal-pane", provider: "Ghostty", badges: ["Surface"] }),
    "Terminal › Ghostty surface"), "Terminal")
  const kinds = [
    "window", "browser-tab", "app-tab", "terminal-tab", "terminal-pane",
    "tmux-session", "tmux-window", "tmux-pane", "herdr-session",
    "herdr-workspace", "herdr-tab", "herdr-pane", "herdr-agent", "neovim-buffer",
  ]
  for (const kind of kinds) {
    const label = sandbox.kindLabel(kind)
    const metadata = sandbox.vitalMetadata(
      item(kind, "Title", "Useful context", { kind, provider: "Provider", badges: [label] }), "")
    assert.notEqual(metadata, label, `${kind} metadata does not repeat its kind`)
  }
  for (const kind of ["herdr-workspace", "herdr-tab", "herdr-pane"]) {
    assert.equal(sandbox.vitalMetadata(
      item(kind, "Child", "Fallback", { kind, badges: [sandbox.kindLabel(kind), "Working"] }),
      "Ancestor › Fallback", "Immediate parent"), "Immediate parent",
    `${kind} metadata is its immediate parent name`)
  }
  assert.equal(sandbox.vitalMetadata(
    item("herdr-agent", "Agent", "Fallback", { kind: "herdr-agent", badges: ["Claude", "Working"] }),
    "Ancestor › Fallback", "Parent pane"), "Working",
  "Herdr agents keep status metadata")
  assert.equal(sandbox.vitalMetadata(
    item("herdr-session", "Session", "Fallback", { kind: "herdr-session", badges: ["Session", "Attached"] }),
    "Fallback", "Ignored parent"), "Attached",
  "Herdr sessions keep session metadata")
}

{
  const hidden = sandbox.withHidden({}, "one")
  const reopened = sandbox.rank([item("one", "One", ""), item("two", "Two", "")], "", hidden)
  assert.deepEqual(Array.from(reopened, row => row.id), ["two"], "hidden ids persist across model refreshes")
}

{
  const rows = [item("one", "One", ""), item("two", "Two", ""), item("three", "Three", "")]
  assert.equal(sandbox.indexOfUuid(rows, "test-uuid:two"), 1)
  assert.equal(sandbox.indexAfterRefresh(rows, "test-uuid:two"), 1,
    "refresh follows the deterministic UI UUID")
  assert.equal(sandbox.indexAfterRefresh(rows, "test-uuid:closed"), 0,
    "a removed thing selects the first row")
  assert.equal(sandbox.indexAfterRefresh([], "test-uuid:two"), -1)
  assert.equal(sandbox.clampContentY(900, 0, 1000, 400), 600)
  assert.equal(sandbox.anchoredContentY(5, 60, 7, 0, 1000, 400), 307)
}

{
  const current = [
    item("one", "One", "", { activationToken: "old-one" }),
    item("two", "Two", "", { activationToken: "old-two" }),
  ]
  const tokenOnlyRefresh = [
    item("two", "Two", "", { activationToken: "new-two" }),
    item("one", "One", "", { activationToken: "new-one" }),
  ]
  const stable = sandbox.reconcileItems(current, tokenOnlyRefresh)
  assert.equal(stable.changed, false, "token-only refresh does not replace the visible model")
  assert.equal(stable.items, current)
  assert.equal(current[0].activationToken, "new-one")
  const changed = sandbox.reconcileItems(current, [
    item("one", "Renamed", "", { activationToken: "latest" }),
    tokenOnlyRefresh[0],
  ])
  assert.equal(changed.changed, true, "a visible field change publishes a new model")
  assert.equal(changed.items[0].title, "Renamed")

  const sameRank = [
    item("one", "One", "", { activationToken: "another-one" }),
    item("two", "Two", "", { activationToken: "another-two" }),
  ]
  assert.equal(sandbox.sameRankedItems(current, sameRank), true,
    "an identical ranked sequence does not update the UI")
  const metadataOnlyRank = [
    item("one", "One", "", { recency: 123, searchTerms: ["latest"], activationToken: "newest-one" }),
    item("two", "Two", "", { recency: 456, searchTerms: ["fresh"], activationToken: "newest-two" }),
  ]
  assert.equal(sandbox.sameRankedItems(current, metadataOnlyRank), true,
    "ranking metadata does not replace an unchanged rendered sequence")
  const metadataRefresh = sandbox.reconcileItems(current, metadataOnlyRank)
  assert.equal(metadataRefresh.changed, true,
    "the source model retains fresh ranking metadata")
  assert.equal(metadataRefresh.items[0].recency, 123)
  assert.equal(sandbox.sameRankedItems(current, sameRank.slice().reverse()), false,
    "a reordered ranked sequence updates the UI")
  assert.equal(sandbox.sameRankedItems(current, changed.items), false,
    "changed rendered traits update the UI")
  const iconChanged = [
    item("one", "One", "", { iconHints: ["new-icon"] }),
    current[1],
  ]
  assert.equal(sandbox.sameRankedItems(current, iconChanged), false,
    "a changed window icon hint updates the rendered row")
}

const qml = fs.readFileSync(path.join(root, "Everything.qml"), "utf8")
const resultListStart = qml.indexOf("id: resultList")
const searchHandler = qml.slice(qml.indexOf("id: searchField"), resultListStart)
const resultDelegateStart = qml.indexOf("\n        delegate: Item", resultListStart)
const listHandler = qml.slice(resultListStart, resultDelegateStart)
const resultDelegateHandler = qml.slice(resultDelegateStart, qml.indexOf("MouseArea", resultDelegateStart))
const barButtonHandler = qml.slice(qml.indexOf("BarIconButton {"),
  qml.indexOf("CenteredPanel {"))
assert.match(barButtonHandler, /useActiveColor:\s*false/,
  "opening the panel does not recolor its bar icon")
assert.doesNotMatch(searchHandler, /Key_Backspace|Key_Delete/, "search owns editing keys")
assert.match(searchHandler, /placeholderText:\s*"Search windows, tabs, apps, menu…"/,
  "the search placeholder uses the requested copy")
assert.match(qml,
  /focusTarget:\s*root\.popupMode === root\.groupsMode \? groupList : searchField/,
  "search remains the results focus target while the checklist targets its list")
assert.match(qml,
  /PanelKeyCatcher\s*\{[\s\S]*blocked:\s*root\.popupMode === root\.resultsMode && searchField\.activeFocus/,
  "the standard shell key catcher yields only while result search is being edited")
assert.match(qml,
  /onMoveRequested:[\s\S]*groupsMode[\s\S]*moveGroupChecklist\(dy\)[\s\S]*moveSelection\(dy\)[\s\S]*dx < 0[\s\S]*collapseCurrentGroup\(\)[\s\S]*dx > 0[\s\S]*expandCurrentGroup\(\)/,
  "the shell's vertical directions move either list while result horizontals fold groups")
assert.match(qml,
  /onActivateRequested:[\s\S]*groupsMode[\s\S]*toggleCurrentGroupVisibility\(\)[\s\S]*activateCurrent\(\)/,
  "the shell's Enter and Space action toggles a checkbox or activates a result")
assert.match(qml,
  /onDeleteRequested:[\s\S]*resultsMode[\s\S]*hideAt\(resultList\.currentIndex\)/,
  "the shell's X action only hides a selected result")
assert.match(qml, /onTabRequested:[^\n]*root\.switchPanel\(direction\)/,
  "the shell owns Tab navigation between panels")
assert.match(listHandler, /Keys\.onPressed[\s\S]*handleSupplementalListKey/,
  "the list retains only supplemental page and control-key aliases")
assert.doesNotMatch(listHandler, /Key_Down|Key_Up|Key_Left|Key_Right|Key_Return|Key_Enter|Key_Tab|Key_Escape/,
  "the list does not override standard shell navigation")
assert.match(qml, /Key_Backspace[\s\S]*Key_Delete[\s\S]*action = "hide"/, "Backspace and Delete hide list rows")
assert.match(qml, /function hideAt[\s\S]*!selectionIsVisibleRow\(index\)[\s\S]*hideItem/,
  "hide keys cannot remove the hidden representative of a collapsed group")
assert.match(qml, /root\.close\(\)[\s\S]*Qt\.callLater[\s\S]*everythingService\.activate/, "panel closes before activation")
assert.match(qml, /onItemsChanged\(\)[\s\S]*scheduleRankedItems\(\)/,
  "provider refresh rebuilds through identity-aware reconciliation")
assert.match(qml, /sameRankedItems\(rankedItems, nextItems\)[\s\S]*return/,
  "an unchanged ranked list is not assigned to the UI")
assert.match(qml, /indexAfterRefresh\(rankedItems, selectedItemUuid/,
  "selection is restored by deterministic UI UUID")
assert.match(qml, /function rowVisualTop[\s\S]*itemAtIndex[\s\S]*delegateItem\.y/,
  "scroll restoration reads the selected delegate's live geometry")
assert.match(qml, /contentHeight > resultList\.height[\s\S]*clampContentY\(selectionVisualTop\(index\) \+ savedSelectedOffset/,
  "scroll offset is restored only when the sectioned list can scroll")
assert.match(qml, /function resetResultsForOpen\(\)[\s\S]*resultUpdateSerial \+= 1[\s\S]*collapsedKinds = \(\{\}\)[\s\S]*applyOpenResultReset\(\)/,
  "opening invalidates stale refresh restores, expands groups, and resets selection")
assert.match(qml, /root\.applyOpenResultReset\(\)[\s\S]*searchField\.forceActiveFocus\(\)/,
  "opening focuses search after resetting the results")
assert.match(qml, /function applyOpenResultReset\(\)[\s\S]*currentIndex = index[\s\S]*contentY = resultList\.originY/,
  "opening selects the first thing and resets the list scroll")
assert.match(qml, /onOpenedChanged:[\s\S]*searchField\.text = ""[\s\S]*rebuildRankedItems\(\)[\s\S]*resetResultsForOpen\(\)/,
  "every results opening resets against the unsearched ranked list")
assert.match(qml,
  /readonly property var supportedGroups:\s*Model\.supportedGroupCatalog\(\)[\s\S]*readonly property var disabledKinds:\s*Model\.normalizeDisabledKinds\([\s\S]*setting\("disabledKinds", \[\]\)/,
  "the checklist and injected settings use the pure canonical model")
assert.match(qml,
  /var nextItems = Model\.rank\([\s\S]*everythingService\.hiddenIds[\s\S]*disabledKinds,\s*focusLeadKind\)/,
  "persistent group visibility participates in every ranked rebuild")
assert.match(qml,
  /function persistDisabledKinds[\s\S]*persistSetting\("disabledKinds", nextKinds\)[\s\S]*root\.settings = entry[\s\S]*updateEntryInline\(root\.moduleName, entry\)/,
  "group changes apply optimistically before shell.json persistence and monitor synchronization")
assert.match(qml,
  /readonly property bool resultsModeOpen:\s*root\.opened && popupMode === resultsMode/,
  "helper leases distinguish results visibility from checklist visibility")
assert.match(qml,
  /onResultsModeOpenChanged:[\s\S]*if \(resultsModeOpen\) everythingService\.acquire\(instanceKey\)[\s\S]*else everythingService\.release\(instanceKey\)/,
  "switching popup modes acquires and releases the results-only helper lease")
assert.match(qml, /PointerMoveGate\s*\{[\s\S]*?referenceItem:\s*resultList[\s\S]*?\}/,
  "row pointer selection uses the shell movement gate")
assert.match(qml, /function selectFromPointer[\s\S]*if \(!pointerMoveGate\.moved\(item, mouse\)\) return[\s\S]*setCurrentIndex\(index, false\)/,
  "pointer selection changes only after measured movement")
assert.match(qml, /function disarmPointer[\s\S]*pointerCursorActive = false[\s\S]*pointerMoveGate\.reset\(\)/,
  "keyboard, opening, and list mutations can disarm pointer selection")
assert.match(qml, /onPositionChanged:[\s\S]*selectFromPointer\(row\.index, row, mouse\)/,
  "real row pointer movement is routed through the movement gate")
assert.doesNotMatch(qml, /rowMouse\.containsMouse/,
  "row styling never treats a stationary pointer as movement")
assert.doesNotMatch(qml, /id:\s*heading\b/,
  "the panel does not render a redundant title row")
assert.match(qml, /readonly property int titlePx:\s*Math\.round\(Style\.font\.body \* fontScale\)/,
  "thing names run at the launcher font scale")
assert.match(qml, /readonly property real rowHeight:\s*Math\.round\(titlePx \* 2\.1\)/,
  "result row height derives from the thing-name font")
assert.match(qml, /section\.property:\s*"kind"[\s\S]*section\.delegate:[\s\S]*Accessible\.Heading/,
  "the list renders accessible kind headings")
const scrollBarHandler = qml.slice(qml.indexOf("ScrollBar.vertical"),
  qml.indexOf("delegate: Item", qml.indexOf("ScrollBar.vertical")))
assert.match(scrollBarHandler, /leftPadding:\s*Style\.space\(4\)[\s\S]*rightPadding:\s*0/,
  "the native scrollbar nub is shifted toward the panel edge")
assert.match(scrollBarHandler, /palette\.mid:\s*Commons\.Color\.accent[\s\S]*palette\.dark:\s*Commons\.Color\.accent/,
  "the native scrollbar nub uses the theme accent")
assert.doesNotMatch(scrollBarHandler, /contentItem:|background:/,
  "scrollbar placement and color do not replace the native control")
assert.match(qml, /id:\s*sectionHeading[\s\S]*readonly property bool collapsed:[\s\S]*Accessible\.focused:\s*highlighted/,
  "a collapsed group leaves its accessible heading as the visible focus target")
assert.match(resultDelegateHandler,
  /height:\s*groupCollapsed \? 0 : root\.rowHeight[\s\S]*visible:\s*!groupCollapsed/,
  "collapsing a group hides its rows without removing its heading")
const groupListStart = qml.indexOf("id: groupList")
const groupDelegateStart = qml.indexOf("\n        delegate: Item", groupListStart)
const groupListHandler = qml.slice(groupListStart, groupDelegateStart)
const groupDelegateHandler = qml.slice(groupDelegateStart)
assert.match(groupListHandler,
  /model:\s*root\.checklistEntries[\s\S]*Accessible\.role:\s*Accessible\.List/,
  "the group panel renders the complete canonical catalog as an accessible list")
assert.doesNotMatch(groupListHandler, /Keys\.onPressed/,
  "the checklist leaves Tab and standard movement to the shell key catcher")
assert.match(groupDelegateHandler,
  /Accessible\.role:\s*Accessible\.CheckBox[\s\S]*Accessible\.checkable:\s*true[\s\S]*Accessible\.checked:\s*checked[\s\S]*Accessible\.onToggleAction:/,
  "every group row exposes checkbox accessibility semantics")
assert.match(groupDelegateHandler,
  /function activate\(\)[\s\S]*setGroupChecklistIndex\(index\)[\s\S]*toggleChecklistEntry\(modelData\)[\s\S]*MouseArea[\s\S]*onClicked:\s*groupRow\.activate\(\)/,
  "pointer input selects and toggles the same checklist row")
assert.match(qml, /import QtQuick\.Effects/,
  "the window image fallback can use the shell-compatible color effect")
assert.match(qml, /id:\s*mappedWindowGlyph[\s\S]*text:\s*row\.windowAppGlyph/,
  "known Omarchy apps render their default-font glyph")
assert.match(qml, /id:\s*applicationIcon[\s\S]*source:\s*row\.windowIconSource/,
  "unmapped window rows display the image resolved from Hyprland identity")
const applicationIconHandler = qml.slice(qml.indexOf("id: applicationIcon"),
  qml.indexOf("id: mappedWindowGlyph", qml.indexOf("id: applicationIcon")))
assert.match(applicationIconHandler, /visible:\s*row\.windowAppGlyph\.length === 0/,
  "a known glyph takes precedence over the application image")
assert.match(applicationIconHandler,
  /MultiEffect[\s\S]*colorization:\s*1\.0[\s\S]*colorizationColor:\s*row\.iconColor/,
  "application images use the same selected and unselected colors as glyphs")
assert.doesNotMatch(applicationIconHandler, /modelData\.active/,
  "the provider's current marker does not brighten a colored application image")
assert.match(qml, /id:\s*fallbackThingIcon[\s\S]*Model\.kindIcon\(row\.modelData\.kind\)/,
  "rows retain a deterministic glyph fallback")
const fallbackIconHandler = qml.slice(qml.indexOf("id: fallbackThingIcon"),
  qml.indexOf("id: metadataLabel", qml.indexOf("id: fallbackThingIcon")))
assert.match(fallbackIconHandler, /color:\s*row\.iconColor/,
  "glyph icons share the row icon color")
assert.match(resultDelegateHandler,
  /readonly property color iconColor:\s*highlighted \? root\.selectedText : root\.dimForeground/,
  "only the actual highlight brightens a row icon")
assert.doesNotMatch(fallbackIconHandler, /modelData\.active/,
  "the provider's current marker does not imitate keyboard highlight")
assert.match(qml, /id:\s*metadataLabel[\s\S]*text:\s*row\.metadata/,
  "every row renders one selected metadata value")
assert.match(qml, /readonly property var itemRelations:\s*Model\.relationIndex/,
  "the panel indexes public parent relationships once per source update")
assert.match(qml, /metadataForItem\([\s\S]*modelData, root\.itemRelations\)/,
  "rendering and sorting derive metadata through the same pure helper")
assert.doesNotMatch(qml, /id:\s*badgeRow|id:\s*badgeLabel|radius:\s*height \/ 2/,
  "result rows do not render metadata pills")
const titleHandler = qml.slice(qml.indexOf("id: titleLabel"), qml.indexOf("MouseArea", qml.indexOf("id: titleLabel")))
assert.doesNotMatch(titleHandler, /font\.bold/,
  "thing names remain regular weight in every state")
assert.match(titleHandler, /font\.weight:\s*row\.emphasis \? Font\.Medium : Font\.Normal/,
  "only an instant answer is set heavier than a thing name")
assert.match(titleHandler, /textFormat:\s*Text\.StyledText[\s\S]*Model\.highlightMarkup\(/,
  "thing names reach the StyledText sink only through the escaping highlighter")
assert.match(titleHandler, /maximumLineCount:\s*1[\s\S]*elide:\s*Text\.ElideRight/,
  "thing names stay on one elided line")
assert.doesNotMatch(resultDelegateHandler, /border\.(?:width|color)/,
  "the highlighted thing uses fill without an outline")
assert.doesNotMatch(qml, /refreshButton|Refresh all providers|everythingService\.refresh\(\)/,
  "automatic discovery does not expose a redundant manual refresh control")
assert.doesNotMatch(qml, /statusText|statusMessage|Starting Everything|Some providers are unavailable|Refreshing…/,
  "the panel renders no discovery or warning messages")
assert.match(qml, /id:\s*emptyState[\s\S]*No matches for/,
  "an empty search says so instead of showing a blank panel")
assert.match(qml, /readonly property var footerHints:[\s\S]*label: "open"[\s\S]*key: "esc"/,
  "a one-line key hint footer sits under the results")
assert.match(qml, /id:\s*footer[\s\S]*model:\s*root\.footerHints/,
  "the footer renders the contextual key hints")
assert.match(qml, /readonly property color foreground:\s*Commons\.Color\.menu\.text/,
  "the card uses the same palette as Omarchy's launcher")
assert.doesNotMatch(qml.replace(/\/\/.*$/gm, ""), /Qt\.darker\(/,
  "secondary text dims with alpha, which also works on light themes")
{
  // Every Text in the panel either declares PlainText or is the one
  // StyledText title fed by Model.highlightMarkup (plugin UI safety).
  const textBlocks = qml.split(/\n\s*Text \{/).slice(1)
  textBlocks.forEach((block, index) => {
    // Look only at this block's own properties, up to its first child or close.
    const body = block.split(/\n\s*\}/)[0]
    if (/Text\.MarkdownText/.test(body)) {
      assert.match(body, /Answers\.safeMarkdown\(/,
        `Text block ${index + 1} renders Markdown only through safeMarkdown`)
      return
    }
    assert.match(body, /textFormat:\s*Text\.(?:PlainText|StyledText)/,
      `Text block ${index + 1} declares an explicit text format`)
  })
}
assert.match(searchHandler, /event\.key === Qt\.Key_Down[\s\S]*root\.moveSelection\(1, true\)/,
  "Down in the search line moves the highlight without leaving the search line")
assert.match(qml, /var retarget = searchField\.text !== lastRankedQuery/,
  "only a changed query re-targets the best match; refreshes keep the selection")
assert.match(qml, /CenteredPanel\s*\{/, "the panel is centered on screen, not hung from the bar icon")
assert.doesNotMatch(qml, /function\s+(?:open|close|togglePanel)\s*\(/,
  "panel uses the inherited shell lifecycle")
assert.match(qml,
  /onPressed:\s*function\(buttonCode\)[\s\S]*buttonCode === Qt\.RightButton[\s\S]*root\.groupsMode : root\.resultsMode/,
  "right-click routes to the checklist while every primary bar action routes to results")
assert.match(qml,
  /function togglePopupMode[\s\S]*root\.opened && popupMode === wanted[\s\S]*root\.close\(\)[\s\S]*if \(root\.opened\)[\s\S]*enterPopupMode\(wanted\)[\s\S]*root\.open\(\)/,
  "same-mode clicks toggle the inherited panel and other-mode clicks switch in place")

const serviceQml = fs.readFileSync(path.join(root, "Service.qml"), "utf8")
assert.match(serviceQml, /reconcileItems\(items, incoming\)/, "token-only snapshots keep the visible model stable")
assert.match(serviceQml, /requestScan\(\)/, "background refreshes remain automatic")
assert.doesNotMatch(serviceQml, /statusMessage|activeScanQuiet/,
  "the service carries no dead panel-message state")
assert.doesNotMatch(serviceQml, /function refresh\s*\(/,
  "the service has no unused manual refresh entry point")

{
  const hl = sandbox.highlightMarkup
  assert.equal(hl("Google Calendar", "cal", "#205ea6"),
    'Google <font color="#205ea6"><b>Cal</b></font>endar',
    "a substring match is emphasised in place")
  assert.equal(hl("Firefox", "ffx", "#205ea6"),
    '<font color="#205ea6"><b>F</b></font>ire<font color="#205ea6"><b>f</b></font>o<font color="#205ea6"><b>x</b></font>',
    "a fuzzy match emphasises the characters the ranker matched")
  assert.equal(hl("devbox: acme-api", "acme api", "#205ea6"),
    'devbox: <font color="#205ea6"><b>acme</b></font>-<font color="#205ea6"><b>api</b></font>',
    "each search term is emphasised separately")
  assert.equal(hl("Café menu", "cafe", "#205ea6"),
    '<font color="#205ea6"><b>Café</b></font> menu',
    "accent folding still points at the original characters")
  assert.equal(hl('<img src="https://example.invalid/t"> & co', "zzz", "#205ea6"),
    '&lt;img src=&quot;https://example.invalid/t&quot;&gt; &amp; co',
    "markup-shaped titles are escaped even without a match")
  assert.equal(hl("<b>x</b>", "x", "#205ea6"),
    '&lt;b&gt;<font color="#205ea6"><b>x</b></font>&lt;/b&gt;',
    "only the highlighter's own tags survive")
  assert.equal(hl("acme-api", "acme", 'red"><img src=x>'),
    "<b>acme</b>-api",
    "a malformed color never reaches the markup")
  assert.equal(hl("Line\nbreak", "", "#205ea6"), "Line break",
    "control whitespace collapses to one line")
  assert.equal(hl("Telegram 😀 bot", "bot", "#123456"),
    'Telegram 😀 <font color="#123456"><b>bot</b></font>',
    "surrogate pairs pass through unsplit")
  assert.deepEqual(Array.from(sandbox.matchPositions("Windows", "tabs")), [],
    "a row found through its group name has no title emphasis")
}

{
  const calc = query => {
    const result = sandbox.evaluateCalculation(query)
    return result ? result.display : null
  }
  assert.equal(calc("2+2"), "4", "plain arithmetic")
  assert.equal(calc("(1 + 2) * 3"), "9", "parentheses bind first")
  assert.equal(calc("2^3^2"), "512", "powers are right-associative")
  assert.equal(calc("-2^2"), "-4", "unary minus binds looser than power")
  assert.equal(calc("2^10"), "1,024", "results group thousands for display")
  assert.equal(sandbox.evaluateCalculation("2^10").copy, "1024", "the clipboard value is ungrouped")
  assert.equal(calc("0.1 + 0.2"), "0.3", "binary float noise is rounded away")
  assert.equal(calc("15% of 80"), "12", "x% of y")
  assert.equal(calc("80 + 15%"), "92", "adding a percent adds that share of the left side")
  assert.equal(calc("80 - 25%"), "60", "subtracting a percent")
  assert.equal(calc("sqrt(16) + abs(-2)"), "6", "functions")
  assert.equal(calc("ln(e)"), "1", "a function call alone is a calculation")
  assert.equal(calc("2pi"), "6.28318530718", "implicit multiplication with constants")
  assert.equal(calc("3 x 4"), "12", "x multiplies")
  assert.equal(calc("7×6"), "42", "the times sign multiplies")
  assert.equal(calc("10!"), "3,628,800", "factorial")
  assert.equal(calc("0xff + 1"), "256", "hex literals")
  assert.equal(calc("10 mod 3"), "1", "mod")
  assert.equal(calc("=pi"), "3.14159265359", "= forces evaluation of a lone constant")
  for (const query of ["2026", "1.5", "pi", "tab 3", "e2e", "ja2-pecel", "workspace 2",
    "covid 19", "100 usd", "1 +", "100/0", "", "constructor(1)", "__proto__(1)",
    "alert(1)", "2+2; rm -rf ~"]) {
    assert.equal(calc(query), null, `${JSON.stringify(query)} is not a calculation`)
  }
  assert.equal(calc("1+".repeat(150) + "1"), null, "overlong input is ignored")
  const answer = sandbox.instantAnswers("6*7", {}).items[0]
  assert.equal(answer.kind, "calc")
  assert.equal(answer.title, "42")
  assert.equal(answer.local.type, "copy")
  assert.equal(answer.local.text, "42")
  assert.equal(sandbox.metadataForItem(answer, {}), "copy", "the answer row explains its action")
  assert.equal(sandbox.instantAnswers("firefox", {}).items.length, 0, "searches get no answer row")
}

{
  const A = sandbox
  const rates = A.validateRates({ base: "EUR", date: "2026-11-06",
    rates: { USD: 1.1165, SGD: 1.4309, IDR: 20208.93, JPY: 176.77, GBP: 0.83415, MYR: 4.5319 } })
  const tz = { "Asia/Singapore": 480, "Asia/Jakarta": 420, "Europe/London": 0, "Asia/Tokyo": 540,
    "America/New_York": -300, UTC: 0 }
  // Fixed instant: 2026-11-07 00:00 UTC (08:00 in Singapore, a Saturday).
  const now = Date.UTC(2026, 10, 7, 0, 0)
  const home = "/home/test"
  const ctx = { rates, currencies: ["SGD", "IDR", "USD"], tzOffsets: tz, localZone: "Asia/Singapore", now,
    home, reminders: [], askLabel: "acme-api",
    clipboard: [{ type: "text", text: "16200" }, { type: "text", text: "hello world\nsecond" },
      { type: "image", path: home + "/.local/state/omarchy/clipboard-images/abc", mime: "image/png", capturedAt: "Wed 22:13" },
      { type: "image", path: home + "/.local/state/omarchy/clipboard-images/../../../.ssh/id", mime: "image/png" },
      { type: "image", path: "/etc/passwd", mime: "image/png" }] }
  const titles = query => Array.from(A.instantAnswers(query, ctx).items, item => item.title)

  // currency
  assert.deepEqual(titles("100 usd"), ["S$128.16", "Rp 1,810,025"], "a bare amount converts to the favourite currencies")
  assert.deepEqual(titles("1.5jt idr to sgd"), ["S$106.21"], "juta and explicit targets")
  assert.deepEqual(titles("rp 50rb in usd"), ["$2.76"], "symbol-first amounts and ribu")
  assert.deepEqual(titles("12*4.5 usd in sgd"), ["S$69.21"], "an expression can be the amount")
  assert.equal(A.instantAnswers("100 usd", ctx).items[0].local.text, "128.16", "money copies as a plain number")
  assert.deepEqual(titles("5 pounds"), ["2.26796 kg"], "pounds is weight, not sterling")
  const noRates = A.instantAnswers("100 usd", Object.assign({}, ctx, { rates: null }))
  assert.equal(noRates.needs.rates, true, "a currency query asks the service for rates")
  assert.match(noRates.items[0].title, /Fetching exchange rates/)

  // units
  assert.deepEqual(titles("5 km in mi"), ["3.10686 mi"])
  assert.deepEqual(titles("100 f to c"), ["37.7778 °C"], "temperature is affine, not a factor")
  assert.deepEqual(titles("2 tb"), ["1.81899 TiB", "2,000 GB"], "default targets per unit")
  assert.deepEqual(titles("5 m"), [], "a bare single-letter unit is too ambiguous")
  assert.deepEqual(titles("5 km in kg"), [], "units of different kinds never convert")

  // time zones
  assert.deepEqual(titles("3pm jakarta"), ["16:00 in Singapore"], "a time in a city is shown in local time")
  assert.deepEqual(titles("15:00 london to tokyo"), ["00:00 in Tokyo (+1d)"], "city to city with day shift")
  assert.deepEqual(titles("3pm in new york"), ["02:00 in New York"], "local time to a city")
  assert.deepEqual(titles("time in tokyo"), ["09:00 in Tokyo"], "current time in a city")
  assert.deepEqual(titles("london time"), ["00:00 in London"])
  assert.deepEqual(titles("3 tokyo"), [], "a bare number is not a time")
  assert.equal(A.instantAnswers("time in tokyo", Object.assign({}, ctx, { tzOffsets: null })).needs.tz, true)

  // timers
  const timer = A.instantAnswers("timer 10m tea", ctx)
  assert.equal(timer.exclusive, true)
  assert.equal(timer.items[0].title, "Remind me in 10 min — tea")
  assert.deepEqual(Array.from(timer.items[0].local.argv), ["omarchy", "reminder", "10", "tea"],
    "the label is one argv entry, never shell text")
  assert.deepEqual(Array.from(A.instantAnswers("remind me in 1h 30m to stretch; rm -rf ~", ctx).items[0].local.argv),
    ["omarchy", "reminder", "90", "stretch; rm -rf ~"], "labels stay inert argv")
  assert.equal(A.instantAnswers("timer app", ctx).items.length, 0, "a non-duration is a normal search")
  assert.equal(A.instantAnswers("timers", ctx).needs.reminders, true)

  // clipboard
  const clip = A.instantAnswers("clip", ctx)
  assert.equal(clip.exclusive, true)
  assert.deepEqual(Array.from(clip.items, item => item.title), ["16200", "hello world second", "Image"],
    "images outside Omarchy's image store are never offered")
  assert.deepEqual(Array.from(A.instantAnswers("clip hello", ctx).items, item => item.local.index), [1],
    "filtering keeps the original history index for the paste helper")
  assert.equal(A.instantAnswers("clip", Object.assign({}, ctx, { clipboard: null })).needs.clipboard, true)

  // ask
  const asked = A.instantAnswers("? what did I spend on grab", ctx)
  assert.equal(asked.exclusive, true)
  assert.equal(asked.items[0].local.type, "ask")
  assert.equal(asked.items[0].local.prompt, "what did I spend on grab")
  assert.equal(A.instantAnswers("?", Object.assign({}, ctx, { lastAsk: { prompt: "hi" } })).items[0].local.type,
    "query", "a bare ? reopens the last answer")
  assert.equal(A.instantAnswers("? x", Object.assign({}, ctx, { disabledKinds: ["ask"] })).items.length, 0,
    "a hidden group yields nothing")

  // agent markdown is neutralised
  assert.equal(A.safeMarkdown('see <img src="https://x/t"> ![a](https://x/p.png)'),
    "see &lt;img src=\"https://x/t\"> [image: a]", "no image or raw HTML reaches the Markdown sink")
  assert.equal(A.safeMarkdown("use `a < b` here"), "use `a < b` here", "code keeps literal characters")
  assert.equal(A.safeMarkdown("```\n<b>\n```"), "```\n<b>\n```")

  // service data parsers
  assert.equal(A.validateRates({ base: "EUR", date: "x", rates: {} }), null)
  assert.equal(A.validateRates({ base: "EUR", date: "2026-11-06", rates: { "US D": 1, A: 2, B: 3, C: 4, D: 5 } }), null,
    "malformed codes reject the whole payload")
  const zones = A.parseZoneOffsets("Asia/Jakarta +0700\nAmerica/New_York -0500\nLOCAL Asia/Jakarta\njunk line\n")
  assert.equal(zones.offsets["America/New_York"], -300)
  assert.equal(zones.localZone, "Asia/Jakarta")
  assert.equal(A.parseZoneOffsets("LOCAL Mars/Base\n").localZone, "UTC", "an unknown local zone falls back to UTC")
  assert.deepEqual(Array.from(A.parseReminders("not json")), [])
  let state = A.emptyAskState()
  state = A.applyAskEvent(state, JSON.stringify({ type: "tool_use", sessionID: "ses_abc123", part: { tool: "read" } }))
  assert.equal(state.tool, "read")
  assert.equal(state.sessionId, "ses_abc123")
  state = A.applyAskEvent(state, JSON.stringify({ type: "text", part: { id: "p1", text: "pong" } }))
  state = A.applyAskEvent(state, JSON.stringify({ type: "step_finish", part: { cost: 0.002 } }))
  state = A.applyAskEvent(state, "garbage")
  assert.equal(state.answer, "pong")
  assert.equal(state.cost, 0.002)
  const running = Object.assign(A.emptyAskState(), { status: "running" })
  const done = A.updatedAskState(running, { status: "done" })
  assert.notEqual(done, running, "a status change is a new object, so QML notifies")
  assert.equal(running.status, "running", "the previous state is never mutated")
  assert.equal(done.status, "done")
  assert.equal(A.applyAskEvent(A.emptyAskState(), JSON.stringify({ sessionID: "ses_; rm -rf" })).sessionId, "",
    "only well-formed session ids are kept")
}

console.log("model.test.js: all tests passed")

{
  const launcher = [
    item("win", "Firefox", "", { kind: "window" }),
    item("app", "Firefox Web Browser", "", { kind: "app" }),
    item("menu", "Style: Theme", "", { kind: "omarchy-menu" })
  ]
  assert.deepEqual(Array.from(sandbox.rank(launcher, "", {}, []), row => row.id), ["win"],
    "launcher groups stay hidden until a query is typed")
  assert.deepEqual(Array.from(sandbox.rank(launcher, "fire", {}, []), row => row.id), ["win", "app"],
    "apps rank after windows once a query is typed")
  assert.deepEqual(Array.from(sandbox.rank(launcher, "theme", {}, []), row => row.id), ["menu"],
    "Omarchy menu routes are searchable by title")
}

{
  const ranked = Array.from(sandbox.rank([
    item("fuzzy", "Firefox", ""), item("exact", "FX Tool", ""), item("miss", "Terminal", ""),
  ], "fx", {}), row => row.id)
  assert.deepEqual(ranked, ["exact", "fuzzy"], "a substring hit outranks a fuzzy subsequence hit")
  assert.deepEqual(Array.from(sandbox.rank([
    item("mag", "Magic Recorder", ""), item("gc", "Google Chrome", ""),
  ], "gc", {}), row => row.id), ["gc", "mag"],
  "word-boundary matches outrank scattered mid-word ones")
}

{
  const rows = [
    item("weak", "adhikasp/herdr-command-plugin: Cmd+K for herdr: fuzzy-find any space, tab, agent, pane or command, with kind prefixes", "", { kind: "browser-tab" }),
    item("strong", "Hacker News", "", { kind: "browser-tab" }),
  ]
  assert.deepEqual(Array.from(sandbox.rank(rows, "hkrnws", {}), row => row.id), ["strong", "weak"],
    "among fuzzy hits the tighter match leads regardless of listing order")
}
