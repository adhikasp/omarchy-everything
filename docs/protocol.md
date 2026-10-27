# JSON-lines protocol

`Service.qml` starts `helper/everything_helper.py --json-lines` as an argv
array. Its path comes from the service component's own resolved file URL,
decoded into a local path; a public-only manifest is sufficient to start it.
UTF-8 JSON objects travel one per line over stdin/stdout. Protocol and
plugin versions are independent: every message has `version: 1`, while the
initial handshake reports plugin version `0.0.1`.

The handshake is the only uncorrelated response:

```json
{"version":1,"type":"ready","pluginVersion":"0.0.1","capabilities":{"partialSnapshots":true}}
```

Every request has a caller-unique `id`. Every response caused by that request
copies it into `requestId`. Unknown requests and provider failures are
nonfatal; the helper continues accepting lines.

## Requests

Scan:

```json
{"version":1,"id":"scan-7","type":"scan","options":{}}
```

`options` is reserved; the helper currently ignores it.

Activate a public item using its process-local opaque token:

```json
{"version":1,"id":"activate-8","type":"activate","itemId":"provider:native-id","token":"v1.opaque"}
```

Shut down cleanly:

```json
{"version":1,"id":"shutdown-9","type":"shutdown"}
```

A newer scan cancels an unfinished older scan: no later partial or full
snapshot is emitted for the cancelled request. Snapshots are emitted only for
explicit scan requests. `Service.qml` supplies the periodic liveness poll, so
every snapshot remains correlated to a current caller ID.

## Responses

Providers stream partial snapshots as they finish. Hyprland is deliberately
published first:

```json
{"version":1,"type":"snapshot","requestId":"scan-7","provider":"hyprland","full":false,"items":[],"warnings":[]}
```

The final snapshot contains the merged view and a provider map:

```json
{"version":1,"type":"snapshot","requestId":"scan-7","full":true,"items":[],"providers":{},"warnings":[]}
```

Activation always completes with an explicit result. `stale: true` means the
native identity closed, was reused, or no longer matches its scan fingerprint:

```json
{"version":1,"type":"activation","requestId":"activate-8","itemId":"provider:native-id","ok":false,"stale":true,"message":"That thing closed before it could be focused"}
```

Malformed input and isolated provider failures use:

```json
{"version":1,"type":"error","requestId":"scan-7","provider":"kitty","message":"Kitty: …","nonfatal":true}
```

A failed provider also publishes an empty partial for its own rows so stale
internal targets disappear; its outer Hyprland window is owned by a different
provider and remains available.

## Item contract

Every public item contains:

| Field | Meaning |
|---|---|
| `id` | Stable provider/native identity, including process birth or socket identity where reuse is possible |
| `uiUuid` | Deterministic UUIDv5 derived from provider, kind, and native identity for UI reconciliation |
| `kind` | One supported actionable thing kind |
| `provider` | Human-readable provider badge; browser and application tabs use their owning application label |
| `title` | Primary display title and the only searchable item field |
| `iconHints` | Ordered, presentation-only icon/application identities; populated for managed windows from Hyprland client data |
| `context` | Cwd, application, workspace, or container context; never searched |
| `searchTerms` | Provider-supplied aliases retained in the contract but not used by panel matching |
| `parentId` | Parent thing used to build breadcrumbs and Herdr child-row metadata |
| `badges` | Status/type annotations used for accessibility and the row's single vital metadata choice; never searched |
| `active` | Whether this is the provider's current thing |
| `recency` | Provider-local ranking hint |
| `activationToken` | Authenticated, process-local opaque activation reference |

Supported kinds are `window`, `browser-tab`, `app-tab`, `terminal-tab`,
`terminal-pane`, `tmux-session`, `tmux-window`, `tmux-pane`, `herdr-session`,
`herdr-workspace`, `herdr-tab`, `herdr-pane`, `herdr-agent`, and
`neovim-buffer`.

Activation bodies never enter QML as a separate field. They are authenticated
inside the token with a helper-process secret. Provider generations make old
tokens stale; activation performs one exact-ID refresh and retry before
returning an error.

`uiUuid` is not an activation credential and carries no mutable runtime state.
The panel compares the final ordered UUID and presentation-trait sequence
before replacing its list model. An identical sequence is a no-op.

## Service lease

Each bar instance owns a unique in-memory results lease. Opening or switching
to results acquires it; closing results or switching to the shell-only group
checklist releases it. A checklist-only popup therefore sends no helper
message. Leases carry the effective palette opt-in (the preference is exactly
true and `terminal-pane` is enabled); any active requesting lease permits the
bridge. Changes to a live lease trigger a new scan, cancelling an obsolete
probe when the last permission is revoked. Opening and helper restart scans
request the bridge only when permitted. The first results lease starts the
helper, and release of the last one
requests shutdown unless an activation is pending. The panel closes before
`Qt.callLater` sends activation, releasing the layer-shell keyboard grab before
another surface receives focus. A helper restart clears queued tokens because
process-local secrets intentionally do not survive restart.
