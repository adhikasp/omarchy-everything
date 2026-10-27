# Testing

## Automated suite

From the repository root:

```bash
tests/all.sh
```

The suite performs:

- case-insensitive contiguous-substring ranking over titles and displayed group
  names only, rejection of subsequences and cross-field queries, explicit
  exclusion of context/provider/badges/search terms, stable kind grouping,
  natural displayed-metadata order with relevance/current/recency tie-breakers,
  the complete ordered 14-group catalog, disabled-kind normalization and
  exclusion, parent-derived metadata through disabled parents, duplicate
  identity handling, token authentication, and hidden-result persistence;
- Hyprland window-to-desktop-entry icon matching, exact Omarchy app-glyph
  precedence, glyph-colored image fallback, generic fallback and Herdr agent
  glyphs, one-line row density, vital-metadata selection,
  JavaScript/Qt-list badge normalization, exclusion of redundant kind labels,
  tmux and Herdr immediate-parent metadata, Neovim containing-directory leaf
  metadata, and absence of visual metadata pills;
- absence of panel scan, count, warning, and empty-result messages while
  automatic discovery continues;
- native list-scrollbar placement with a right-shifted nub using the current
  theme accent palette;
- invariant bar-icon foreground color while the panel is active;
- every documented keyboard alias, horizontal group collapse/expand, vertical
  skipping of hidden rows, focus-sensitive Delete/Backspace ownership, safe
  selection after removal, keyboard-to-first scroll reset, stationary-pointer
  filtering, deliberate pointer selection, and close-before-activation
  ordering;
- argv injection rejection, command timeout/kill behavior, JSON-lines request
  correlation, malformed-request recovery, and stale-token rejection;
- partial provider streaming, provider failure isolation, stale-row removal,
  and scan cancellation propagation;
- Hyprland mapped/hidden/group/scratchpad handling, mapped-only child-provider
  routing with unmapped matching evidence, and Herdr feature discovery of
  every supported kind, parent links, and exact focus targets using the
  synthetic `tests/fixtures/herdr-snapshot.json` response. Herdr tests cover
  older, future, and absent version/protocol labels; extra fields; independently
  missing/malformed collections; invalid or duplicate native IDs; optional
  ranking metadata; unavailable snapshots; and focus failures before routing;
- `omarchy plugin validate .` against the installed current manifest schema.

## Live helper smoke test

To inspect live discovery without activating anything, start the helper,
send one `scan` request, read partials until the full snapshot, then send
`shutdown`. Confirm afterward that `org.a11y.Status.IsEnabled` equals its value
before the run. The automated protocol tests use `--test-mode`; live smoke
testing intentionally does not.

Useful observation commands are:

```bash
hyprctl -j clients
omarchy-shell shell listPlugins
omarchy menu keybindings --print
```

## Manual acceptance matrix

Install the plugin into a disposable user plugin checkout, enable it, and run
these cases on the current Omarchy 4 package set. For each case verify both the
row/breadcrumb and exact activation target, then close the target and verify a
single refresh followed by a visible stale notification.

### Shell and windows

- Multiple monitors: invoke the shell shortcut while each monitor is focused;
  only that monitor's widget opens. Open two panels by pointer to confirm the
  helper remains leased until both close.
- Right-click the bar icon before opening results. Confirm the checklist shows
  all 14 supported groups in result-heading order, every row is checked even
  when that kind has no discovered things, and no discovery helper starts.
  Use Up/Down and pointer clicks to move, Enter/Space to toggle, Tab to follow
  shell panel navigation, and Escape to close. While it remains open,
  left-click the icon and confirm it switches in place to results and acquires
  a lease; right-click again and confirm it switches back and releases it.
- Uncheck groups and confirm their rows disappear immediately without changing
  transient per-thing hiding or per-opening collapse. Disable a parent group
  while leaving a child group enabled and confirm the child keeps its parent
  metadata. Disable all groups and confirm results remain intentionally empty,
  then recheck them and confirm the rows return. Open the checklist on a second
  monitor and confirm changes synchronize. Reopen the panel, reload the plugin,
  and restart the shell to confirm the choices persist; a newly supported group
  must start checked.
- Confirm search owns initial focus and printable input edits it immediately.
  Press Down or Tab to enter navigation, then confirm K/J move backward/forward
  like Up/Down and H/L collapse/expand like Left/Right. Enter/Space activates,
  X hides, and Tab follows Omarchy's adjacent-panel navigation. Press `/` to
  return to search and confirm H, J, K, and L insert text instead of moving the
  selection. Scroll down, then reach the first result with Up, K, Home, or `g`;
  confirm the list returns fully to the top.
- Managed clients across numbered workspaces, a special scratchpad, a group,
  hidden state, and duplicate titles. Confirm layers do not appear.
- Confirm Foot and Alacritty expose their exact separate Hyprland windows but
  no fabricated native tabs/panes.
- Start `btop`, ordinary shells, and background processes. Confirm none becomes
  a row except through a window, pane, or supported container.

### Kitty

- Kitty: multiple OS windows, tabs, panes, duplicate titles, a removed pane,
  and a stale runtime socket. Confirm numeric IDs select the intended object
  before its exact outer window is focused.

### tmux, Herdr, and Neovim

- tmux: default and custom sockets, attached and detached sessions, multiple
  clients, a remote-only client, immutable duplicate titles, and closed
  targets. Confirm only one unique local client is switched; otherwise a fresh
  local attach opens.
- Herdr: multiple running local sessions exposing native snapshots, detached sessions,
  workspaces/tabs/panes, and agents in idle/working/blocked/done states. Confirm
  exact socket focus methods and fresh attach behavior for ambiguous hosts.
- Neovim: displayed, hidden, loaded, unloaded, and modified listed buffers;
  existing windows on another tab; standalone, tmux-nested, and Herdr-nested
  servers. Confirm a displayed buffer selects its first existing window and
  never starts a new terminal attach or remote UI; confirm `:hide buffer`
  preserves modified buffers. Confirm every named buffer shows only the final
  component of its canonical containing directory and an unnamed buffer shows
  only the final component of the editor working directory, regardless of
  modified/loaded/displayed status badges.

### Lifecycle and recovery

- With a middle row highlighted, refresh an unchanged result set and confirm
  the list model is not replaced. Then reorder or add rows and confirm the
  same UUID stays highlighted at the same visual offset. Remove that row and
  confirm selection returns to the first result. Repeat with a short list that
  cannot scroll and confirm only the highlight is preserved.
- Populate several kinds at once. Confirm each exact kind has one accessible
  heading, groups remain in stable order, and ranking is preserved within each
  group. Confirm every Herdr group precedes the tmux groups, Herdr agents
  precede every other Herdr group, and Herdr sessions follow the other Herdr
  groups. Refresh while the selected row crosses a heading and
  confirm its visual offset is retained. Within a group, confirm the displayed
  metadata controls natural ordering, including `Workspace 2` before
  `Workspace 10`, Scratchpad after regular window workspaces, and Herdr child
  rows ordered by their displayed parent name. Confirm tmux windows show only
  their session name and tmux panes show only their window name; neither value
  includes native IDs, socket paths, cwd chains, or deeper ancestry.
- Highlight a row in the middle of a group and press Left or H. Confirm its
  rows collapse, its accessible heading keeps the highlight, vertical movement
  skips directly to an adjacent visible row or collapsed heading, and removal
  keys do nothing while the heading is selected. Press Right or L and confirm
  the same row returns. Close and reopen the panel and confirm all groups are
  expanded.
- Confirm every row is one line with an icon on the left and exactly one
  status or context value on the right. Exercise long titles and paths, active
  states, modified buffers, detached sessions, and special workspaces; confirm
  a known window app uses Omarchy's Nerd Font glyph, an unmapped window image
  is recolored to the same selected/unselected foreground, Herdr agents use the
  shell's agent robot, buffers show only their directory leaf, and unresolved
  windows use the window fallback. Confirm text elides without overlap, kind
  labels never repeat in the right-side value, and no pill-shaped metadata is
  rendered.
- Scroll to and highlight a middle result, close the panel, and open it again.
  Confirm the first result is highlighted and the list is at its scroll origin.
- Open the panel while the pointer is stationary over a result. Confirm that
  the pointer does not steal the keyboard highlight; move it deliberately and
  confirm the highlight follows exactly one row. Repeat across a list refresh.
- Hide rows, close/reopen the panel, and wait for an automatic refresh: they
  remain hidden. Reload `b.everything` or `omarchy-shell`: they return.
