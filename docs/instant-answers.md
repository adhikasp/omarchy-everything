# Instant answers

Owner: `InstantAnswers.js` (pure parsing and formatting) and the
"instant-answer data" block of `Service.qml` (all side effects).
`Everything.qml` calls `instantAnswers(query, context)` on every rank and
prepends the returned rows, or replaces the results with them for prefix modes.

## Kinds

| Kind | Trigger | Enter |
| --- | --- | --- |
| `calc` | an expression with an operator or function, or a leading `=` | copy the plain value |
| `convert` | `100 usd`, `1.5jt idr to sgd`, `5 km in mi`, `100 f to c` | copy the plain value |
| `time` | `3pm jakarta`, `15:00 london to tokyo`, `3pm in new york`, `time in tokyo` | copy `HH:MM` |
| `timer` | `timer 10m tea`, `remind me in 1h 30m to stretch`, `timers` | `omarchy reminder <min> [label]` / `clear` |
| `clipboard` | `clip`, `cb`, `clipboard` + optional filter | paste via Omarchy's paste helper |
| `ask` | `? question` or `ask question` | run `opencode run --format json` in the workspace |

Prefix modes (`ask`, `clipboard`, `timer`) are exclusive: they replace search
results. A non-duration after `timer` falls back to a normal search. Every kind
is a search-only group, so it can be hidden from the right-click checklist.

## Settings (`shell.json` plugin entry)

- `askWorkspace` (default `~`): directory `opencode run` starts in.
- `askModel` (optional): `provider/model` passed as `--model`.
- `currencies` (default `["USD", "EUR"]`): targets for a bare amount.

## Data the service supplies (`needs`)

- `rates`: ECB reference rates from `https://api.frankfurter.dev/v1/latest?base=EUR`,
  fetched only when a currency query is typed and the cache is older than 12 h,
  with an 8 s timeout, a 20 KB response cap and whole-payload validation
  (`validateRates`). Cached at `~/.cache/omarchy-everything-rates.json`. The
  request carries no user data.
- `tz`: today's UTC offset per known zone from tzdata (`TZ=<zone> date +%z`),
  zone names passed as argv; refreshed at most every 30 min. Offsets are
  today's, so conversions across a DST change can be off by an hour.
- `clipboard`: Omarchy's `~/.local/state/omarchy/clipboard-history.json`, watched
  only after a clipboard query. Image rows are offered only for files under
  Omarchy's clipboard image directory with an image MIME type.
- `reminders`: `omarchy reminder show --json`, refetched after any timer action.

## Ask

`Service.ask()` runs `sh -c 'exec "$@" < /dev/null' sh opencode run --format json
--title … <prompt>` in the workspace. stdin must be `/dev/null`: `opencode run`
reads a non-TTY stdin to EOF and would hang. The prompt is never preceded by `--`
(opencode 2.2 then sends it twice); a prompt starting with `-` gets a leading
space. JSON events are folded by `applyAskEvent`; state objects are replaced,
never mutated, because QML only notifies a `var` property for a new object.
The answer renders as Markdown only after `safeMarkdown` strips images and
escapes raw HTML outside code. Ctrl+O reopens the session with
`opencode --session <id>` in the workspace, preferably as a new tab in the
selected space of the [Herdr](https://herdr.dev) desktop app
(`herdr-app do tab.new`, a tab named `ask · <prompt>`), so the conversation
continues in the same terminal surface as the other agents; when `herdr-app`
is missing or its window is closed the same command falls back to
`xdg-terminal-exec opencode --session <id> <workspace>`. If no panel is open
when the answer lands, it is delivered as a notification. Timeout: 5 min.

## Verification

`tests/model.test.js` covers every parser with fixed rates, offsets and clock,
hostile Markdown, malformed service payloads and argv-inertness of timer labels.
