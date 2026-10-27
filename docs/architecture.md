# Architecture

## System shape

The dependency direction is:

```text
Everything.qml -> Service.qml -> JSON lines -> helper (Rust) -> providers
       |               |                              |
       v               v                              v
EverythingModel.js  public items               local native APIs
```

Providers do not depend on QML. QML does not receive provider activation
structures or call native tools directly. See [the protocol](protocol.md) for
the wire schema and [provider behavior](providers.md) for adapter guarantees.

## Frontend ownership

`Everything.qml` is one bar instance per monitor. It owns the panel mode,
search focus, highlighted UUID, collapsed-kind map, checklist selection, list
scroll state, accessibility, and close-before-activation ordering. It uses the
shell's `KeyboardPanel`, icon anchoring, and `PanelKeyCatcher`; shell navigation
remains authoritative after
search hands focus to the list. Vertical directions move between visible rows
or collapsed headings, while horizontal directions collapse or expand the
current group. A collapsed group retains its heading as the visible accessible
focus target and never exposes a hidden row to activation or removal. Each
opening clears and focuses search, expands every group, selects the first ranked
thing, and resets the list to its scroll origin. UUID and visual-offset
preservation applies only to refreshes while that opening remains active.
Any keyboard path that reaches the first ranked thing also resets the list to
its scroll origin; pointer selection and refresh reconciliation do not.
Pointer-driven selection passes through the
shell's `PointerMoveGate`; opening, keyboard navigation, and list mutations
reset that gate so delegates appearing beneath a stationary pointer cannot
change the highlighted thing.
The bar chevron remains inside the shell's standard `BarIconButton` slot. Its
painted bounds are optically centered without shifting the widget geometry,
pointer target, or `KeyboardPanel` anchor.
The presentation renders no status row; scan progress, empty searches, counts,
and provider warnings never become panel messages. Activation failure can still
use the shell's external notification path after the panel closes.

The primary bar action opens or toggles results. Right-click opens or toggles a
checklist containing the complete supported group catalog, and either action
switches an already-open popup in place when it targets the other mode. The
checklist is useful without discovery, so only results mode owns a helper
lease. Its normalized `disabledKinds: string[]`, containing exact protocol kind
identifiers, is global widget configuration:
the active instance updates its injected `settings` optimistically and writes
the merged entry through the shell's `updateEntryInline` API. The shell owns
`shell.json` persistence and injects the change into every monitor instance.
Missing, malformed, duplicate, and unknown values are ignored; consequently a
new catalog group is enabled until explicitly disabled. This persistent filter
is independent of per-opening group collapse and the service's transient
per-thing hidden IDs.

## Helper and provider ownership

Native discovery stops at every `DOCUMENT_*` root instead of spending the
global node budget inside content that cannot own native controls. Pinta and
Nautilus activation revalidates the previously discovered strip path before
and after its GTK action; the path must still avoid document roots and resolve
to the same strict tab-list structure, count, index, and native ID. Unrelated
application branches are not rediscovered during activation.

## Identity and activation

Three identities serve different boundaries:

| Value | Purpose | Lifetime |
| --- | --- | --- |
| `Thing.id` | Stable native provider identity | Native target lifetime |
| `uiUuid` | Deterministic UI reconciliation identity | Same stable traits |
| `activationToken` | Authenticated reference to private activation data | Helper process and provider generation |

## Lifecycle and state

- The helper exists only while at least one results-mode lease or activation
  is active. Closing or switching away from the last results panel requests
  shutdown after pending activation; checklist-only popups never start it.
- Hyprland rows can arrive before slower adapters; `Service.qml` merges
  provider partials rather than replacing unrelated providers.
- The panel closes before activation is dispatched so the shell releases its
  keyboard grab before another local surface receives focus.

Detailed message and lease behavior belongs in [the protocol](protocol.md).
Runtime authority and cleanup belong in [security](security.md).

## Invariants

- Only actionable things are results: managed windows, native tabs/panes,
  supported containers and agents, and Neovim buffers. Processes remain
  private routing metadata.
- This is an Omarchy Quattro plugin, not a Codex plugin. Current packaged
  interfaces are the only compatibility target.
- Default shell placement, anchoring, panel lifecycle, and keyboard navigation
  are preserved; optical icon corrections cannot move the shell slot, pointer
  target, or panel anchor.
- Provider commands use argv arrays with bounded timeouts and no shell
  interpolation. Untrusted runtime data is revalidated at activation.
- Provider failures are isolated and nonfatal. Ambiguous native routing fails
  closed or uses the documented fresh-client behavior instead of guessing.
- No network request, privileged operation, install hook, persistent worker,
  or on-disk hidden-state store is introduced.

See [security](security.md) for the exhaustive authority list and
[testing](testing.md) for the verification matrix.

## Evolving boundaries

When a change alters a layer's owner, inputs/outputs, dependency direction,
lifetime, trust assumptions, or failure semantics, update this document and
the specialized document in the same change. A new boundary gets a focused
page under `docs/`, linked here and from `../AGENTS.md`; document its owner,
contract, invariants, failure behavior, and tests.
