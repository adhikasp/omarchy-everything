# Security and runtime access

Everything is local-only. Runtime code performs no HTTP, DNS, telemetry,
update, or other network request. The plugin has no install hook, privileged
operation, `sudo`, browser extension, native-messaging host, persistent daemon,
or background worker after its last service lease closes. Installation may use
Omarchy's normal Git-based plugin manager; that fetch is not performed by the
plugin.

All subprocesses are created from argv arrays through
`asyncio.create_subprocess_exec`, Quickshell `Process.command`, or
`Quickshell.execDetached`. No command is passed through a shell. NUL and line
breaks are rejected in argv values, and foreground commands have bounded
timeouts. External provider failure is nonfatal.

## Commands

The helper may execute only these command families:

- `hyprctl -j clients` and `hyprctl -j activewindow` for managed-client state.
- `hyprctl dispatch <Lua expression>` for exact-address `hl.dsp.focus` and the
  targeted `hl.dsp.send_key_state` cases documented below.
- `kitten @ --to unix:<validated-socket> ls`, `focus-tab --match id:<number>`,
  and `focus-window --match id:<number>`.
- `tmux -S <validated-socket> list-sessions`, `list-windows -a`,
  `list-panes -a`, `list-clients`, and `switch-client`; or a new local
  `omarchy-launch-terminal tmux -S … attach-session …` client.
- `herdr session list --json`; Herdr focus itself uses its validated Unix
  socket. Detached/ambiguous targets may run
  `omarchy-launch-terminal herdr session attach <name>`.
- `nvim --server <validated-socket> --remote-expr <fixed expression>` to read
  or switch a buffer; ambiguous hosts may run
  `omarchy-launch-terminal nvim --server <socket> --remote-ui`.
- `$OMARCHY_PATH/bin/omarchy-notification-send --app-name Everything …` after
  a stale or failed user activation.

Arguments derived from titles, paths, sockets, or native IDs remain individual
argv fields. The fixed Neovim expression receives only a validated integer
buffer number; names are rechecked, not interpolated into Vimscript.

## Local sockets

The following Unix sockets may be opened after ownership and socket-type
checks:

- Kitty: `$XDG_RUNTIME_DIR/omarchy-kitty-<PID>`; device/inode, ownership,
  socket type, PID, and process birth are revalidated before activation.
- tmux: entries below `/tmp/tmux-<UID>`, tmux-named entries directly below
  `$XDG_RUNTIME_DIR`, and absolute custom `-S` paths found in same-user argv.
- Herdr: absolute paths returned by local `herdr session list --json`.
- Neovim: `nvim*` sockets directly below `$XDG_RUNTIME_DIR` or one private
  tempdir beneath it, entries beneath `${TMPDIR:-/tmp}/nvim.<username>`, and
  absolute `--listen` paths from same-user Neovim argv. Traversal is limited to
  one directory level and 4096 entries per root; the fixed RPC query confirms
  the socket's current-user Nvim PID.

Herdr requests are newline-delimited JSON methods `session.snapshot`,
`workspace.focus`, `tab.focus`, and `pane.focus`, with bounded
connect/read/write timeouts and response size. Snapshot structure and unique,
nonempty string native IDs establish usable targets; release/protocol labels
are not trust checks. Invalid or ambiguous native IDs never reach a focus
request. Discovery sends no mutating focus probes, and activation requires a
successful result before terminal routing.

## `/proc` reads

`/proc` is used only as same-user routing metadata. No process becomes a result
because it exists. The helper reads:

- `/proc/<pid>/stat` for parent PID and immutable process start tick;
- `/proc/<pid>/cmdline` for executable identity plus explicit tmux `-S` and
  Neovim `--listen` paths;
- `/proc/<pid>/cwd` for canonical routing context;
- up to 256 KiB of `/proc/<nvim-pid>/environ`, retaining only
  `HERDR_SOCKET_PATH` and `HERDR_PANE_ID`, to identify an inherited exact
  Herdr pane when several panes share a working directory;
- `/proc/net/unix` and `/proc/<herdr-server-pid>/fd` symlinks to associate a
  listed Herdr socket with its server process;
- directory ownership from `/proc/<pid>` to exclude other users.

Reads are bounded to numeric processes owned by the current UID. An oversized
environment read is discarded completely. Ordinary process names, command
lines, and environment values are never serialized into the public result
list.

## Persistence