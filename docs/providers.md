# Provider behavior

## Hyprland windows

Window presentation also retains the client's ordered icon hint, class,
initial class, and initial title. QML resolves those Hyprland-reported values
against the shell's current desktop entries, including the host/path identity
used by Omarchy web apps. An exact identity in Omarchy's current default app
map uses its Nerd Font glyph. Other clients use the resolved desktop-entry or
direct-hint image, recolored to the same foreground as a glyph; an unresolved
client uses the generic window glyph. Entry-only identities are never inferred
from a raw class, and web apps do not inherit their host browser's glyph. These
hints are display-only and never participate in activation identity.

Everything has no internal Linux tab/split adapter for Foot or Alacritty, so
their only native things are their exact managed windows. tmux,
Herdr, and Neovim nested inside them are still expanded. This
matches [Alacritty's explicit non-goal of providing tabs or splits](https://github.com/alacritty/alacritty#faq).

## Chrome tabs

Chrome and Chromium tabs come from the Screen Context extension's native-host
socket (`$SCREEN_CONTEXT_SOCK`, default `$XDG_RUNTIME_DIR/screen-context/host.sock`).
Without that extension the browser appears only as its Hyprland window.
Activation switches to the exact tab and focuses its window.

## Kitty

Only current-user sockets named `$XDG_RUNTIME_DIR/omarchy-kitty-<PID>` are
accepted. Socket ownership, type, live Kitty PID, and process start tick are
validated before `kitten @ … ls` enumerates OS windows, tabs, and Kitty windows
(panes). Numeric tab/window IDs are activated with `focus-tab` or
`focus-window`, then the exact matching Hyprland client is focused. Socket and
process identity—including socket device/inode and process birth—are part of
every ID and are revalidated to prevent PID/socket reuse. Command construction
follows Kitty's [remote-control protocol](https://sw.kovidgoyal.net/kitty/remote-control/).

## tmux

Discovery covers same-user sockets in `/tmp/tmux-<UID>`, tmux-named runtime
entries, and explicit `-S` sockets found in same-user process argv. Sessions,
windows, and panes use immutable `$`, `@`, and `%` native IDs. A target with one
uniquely matched local client uses `switch-client` and focuses that client's
terminal. Detached, remote-only, or ambiguous-host targets open a fresh local
terminal attached directly to the session and target. Existing remote clients
are never selected as hosts.

The line metadata stays structural and compact: sessions show Attached or
Detached, windows show their session name, and panes show their window name.
Native IDs, socket paths, cwd chains, and deeper ancestry remain available to
the provider for identity and routing but are not rendered in that value.

## Herdr

`herdr session list --json` supplies running local session sockets. Everything
accepts only absolute current-user Unix sockets, requests a
`session.snapshot`, and requires a `session_snapshot` result with an object
payload. Version and protocol fields are informational and may be absent.
The returned workspace, tab, pane, and agent collections determine which
features are available. An absent collection omits that kind; a malformed
collection or row produces a nonfatal warning without suppressing the other
features or sessions. Every workspace/tab/pane target must have a nonempty
string native ID without NULs or line breaks, unique within its collection.
Agent targets require the same unambiguous pane ID. Extra fields and absent
optional display/ranking metadata do not prevent discovery. The parent graph
uses the immediate parent when available, falling back to the session when
that parent collection is unavailable.

Activation sends exact `workspace.focus`, `tab.focus`, or `pane.focus` socket
requests with those native IDs. Discovery never issues focus commands as
capability probes. An unavailable focus method, native error, or missing result
fails activation before terminal routing. One unambiguous attached local client
is focused; detached or ambiguous-host sessions open a fresh
`herdr session attach`. Remote sessions are not discovered.

## Neovim

Everything accepts current-user Unix sockets in the current runtime directory
or its one-level private tempdir, `${TMPDIR:-/tmp}/nvim.<username>`, or explicit
live `--listen` argv. Discovery is same-user, non-symlink, and entry bounded; a
fixed `--remote-expr` confirms the owning Nvim PID and reads listed buffers plus
changed, loaded, displayed, current, and last-used state.
Named buffer rows use the filename as their title and its canonical containing
directory as their display context. An unnamed buffer uses the editor's
canonical working directory. Status badges remain available to accessibility
and activation continues to use the separately revalidated canonical name.
The presentation model reduces that context to its final path component for
the row's metadata; routing retains the complete canonical path.

## Other applications

Every other managed window remains searchable: this includes current Omarchy
applications such as Nautilus, Obsidian, LibreOffice, Kdenlive, OBS, Pinta,
Xournal++, Evince, LocalSend, Moonlight, mpv/imv, OMA apps, and optional
editors. Strict externally actionable native tabs may add rows, but an adapter
failure never removes the outer-window fallback.
