from __future__ import annotations

import asyncio
import json
import os
import socket
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch

from everything.atspi_runtime import _AT_SPI_CANCEL_EVENT
from everything.commands import CommandError, CommandResult
from everything.processes import ProcTable, ProcessInfo
from everything.providers.atspi import NativeTab, TopLevel
from everything.providers.base import ScanContext, route_for_process
from everything.providers.ghostty import (
    Atspi,
    EffectiveBinding,
    GhosttyProvider,
    PaletteFingerprint,
    PaletteState,
    PaletteUnavailable,
    parse_keybindings,
)
from everything.providers.hyprland import HyprlandProvider
from everything.providers.hyprland import launch_terminal_and_focus


class FakeRunner:
    def __init__(self, stdout: str) -> None:
        self.stdout = stdout
        self.calls: list[tuple[str, ...]] = []

    async def run(self, argv, **_kwargs):
        command = tuple(str(value) for value in argv)
        self.calls.append(command)
        return CommandResult(command, 0, self.stdout, "")


class HyprlandTests(unittest.IsolatedAsyncioTestCase):
    async def test_only_mapped_clients_become_items_and_shared_routes(self) -> None:
        clients = [
            {
                "address": "0xabc",
                "mapped": True,
                "hidden": True,
                "icon": "foot-symbolic",
                "class": "foot",
                "initialClass": "foot",
                "initialTitle": "foot_/",
                "title": "shell",
                "pid": 999999,
                "workspace": {"id": -99, "name": "special:scratch"},
                "grouped": ["0xdef"],
                "focusHistoryID": 4,
            },
            {"address": "0xdef", "mapped": False, "class": "ignored", "pid": 1},
        ]
        context = ScanContext(FakeRunner(json.dumps(clients)), ProcTable({}))  # type: ignore[arg-type]
        result = await HyprlandProvider().scan(context)
        self.assertEqual(len(result.items), 1)
        self.assertEqual(result.items[0].kind, "window")
        self.assertIn("Hidden", result.items[0].badges)
        self.assertIn("Scratchpad", result.items[0].badges)
        self.assertEqual(result.items[0].icon_hints, ["foot-symbolic", "foot", "foot_/"])
        self.assertNotIn("process", result.items[0].kind)
        self.assertEqual(context.hypr_clients, [clients[0]])
        self.assertEqual(context.hypr_matching_clients, clients)








class GhosttyBindingTests(unittest.TestCase):
    def test_effective_bindings_are_parsed_without_shell_text(self) -> None:
        parsed = parse_keybindings(
            "keybind = alt+1=goto_tab:1\n"
            "keybind = ctrl+tab=next_tab\n"
            "keybind = ctrl+shift+p=toggle_command_palette\n"
            "keybind = global:ctrl+p=toggle_command_palette\n"
            "keybind = ctrl+a>2=goto_tab:2\n"
        )
        self.assertEqual(parsed["goto_tab:1"], [EffectiveBinding("ALT", "1")])
        self.assertEqual(parsed["next_tab"], [EffectiveBinding("CTRL", "TAB")])
        self.assertEqual(
            parsed["toggle_command_palette"], [EffectiveBinding("CTRL + SHIFT", "P")]
        )
        self.assertNotIn("goto_tab:2", parsed)


class PaletteWalker(GhosttyProvider):
    def __init__(self, rows: list[tuple[str, str]]) -> None:
        super().__init__()
        self.rows = rows
        self.index = 0

    async def _wait_selected(self, _palette):
        return self.rows[self.index] if self.index < len(self.rows) else None

    @staticmethod
    def _row_values(accessible):
        return accessible

    @staticmethod
    def _row_marker(accessible):
        return "|".join(accessible)

    async def _send_verified_key(self, _context, _address, _palette, key, mods=""):
        del mods
        if key == "DOWN":
            self.index += 1

    async def _wait_row_change(self, _palette, _marker):
        return self.index < len(self.rows)


class FakePaletteAccessible:
    def __init__(self, role_value, name: str, *children, editable: bool = False) -> None:
        self.role_value = role_value
        self.name = name
        self.children = list(children)
        self.editable = editable
        self.text = ""
        self.accept_text = True
        self.action = None
        self.parent = None
        self.path = "fake:" + str(id(self))
        for child in self.children:
            child.parent = self

    def get_role(self):
        return self.role_value

    def get_name(self):
        return self.name

    def get_description(self):
        return ""

    def get_child_count(self):
        return len(self.children)

    def get_child_at_index(self, index):
        return self.children[index]

    def get_parent(self):
        return self.parent

    def get_index_in_parent(self):
        return self.parent.children.index(self) if self.parent else 0

    def get_state_set(self):
        return type("State", (), {"contains": lambda _self, _value: False})()

    def get_editable_text_iface(self):
        return self if self.editable else None

    def get_text_iface(self):
        return self if self.editable else None

    def get_text(self, _start, _end):
        return self.text

    def set_text_contents(self, value):
        if self.accept_text:
            self.text = value
        return True

    def get_action_iface(self):
        return self if self.action else None

    def get_n_actions(self):
        return 1

    def get_action_name(self, _index):
        return "win.toggle-command-palette"

    def do_action(self, _index):
        return self.action()

    def get_accessible_id(self):
        return self.path


class FakeGhostty:
    def __init__(self, *, editable=True, accept_text=True):
        self.search = FakePaletteAccessible(Atspi.Role.ENTRY, "", editable=editable)
        self.search.accept_text = accept_text
        branch = self.search
        for _ in range(13):
            branch = FakePaletteAccessible(Atspi.Role.PANEL, "", branch)
        self.list_view = FakePaletteAccessible(
            Atspi.Role.LIST, "", FakePaletteAccessible(Atspi.Role.LIST_ITEM, "Focus: Shell")
        )
        self.dialog = FakePaletteAccessible(Atspi.Role.DIALOG, "", branch, self.list_view)
        self.frame = FakePaletteAccessible(Atspi.Role.FRAME, "Shell")
        self.frame.action = self.toggle
        self.application = FakePaletteAccessible(Atspi.Role.APPLICATION, "Ghostty", self.frame)
        self.opened = False
        self.toggles = 0
        self.client = {"pid": 17, "address": "0xabc", "class": "com.mitchellh.ghostty"}
        self.top = TopLevel(self.application, self.frame, 17, 0, "Shell", self.client)
        self.tabs = [NativeTab(self.frame, self.top, (), (), 0, "Shell", True, "tab-1")]

    def toggle(self):
        self.toggles += 1
        self.opened = not self.opened
        self.application.children = [self.frame, self.dialog] if self.opened else [self.frame]
        self.dialog.parent = self.application if self.opened else None
        return True

    def context(self, *, allow=True, refresh=True):
        return ScanContext(FakeRunner(""), ProcTable({}),
                           hypr_clients=[self.client], include_ghostty=refresh,
                           allow_ghostty_palette=allow)


class GhosttyPaletteTests(unittest.IsolatedAsyncioTestCase):
    @unittest.skipIf(Atspi is None, "PyGObject AT-SPI is unavailable")
    async def test_palette_search_and_list_must_share_a_modal(self) -> None:
        row = FakePaletteAccessible(Atspi.Role.LIST_ITEM, "Focus: Surface")
        list_view = FakePaletteAccessible(Atspi.Role.LIST, "Commands", row)
        search = FakePaletteAccessible(
            Atspi.Role.ENTRY, "Execute a command", editable=True
        )
        dialog = FakePaletteAccessible(Atspi.Role.DIALOG, "Command Palette", search, list_view)
        application = FakePaletteAccessible(Atspi.Role.APPLICATION, "Ghostty", dialog)
        palette = GhosttyProvider._find_palette(application)
        self.assertIsNotNone(palette)
        self.assertIs(palette.search, search)  # type: ignore[union-attr]
        self.assertIs(palette.list_view, list_view)  # type: ignore[union-attr]

        frame = FakePaletteAccessible(Atspi.Role.FRAME, "Terminal", search, list_view)
        application = FakePaletteAccessible(Atspi.Role.APPLICATION, "Ghostty", frame)
        self.assertIsNone(GhosttyProvider._find_palette(application))

    async def test_virtualized_rows_and_duplicate_occurrences_are_complete(self) -> None:
        rows = [(f"Focus: Surface {index}", f"/work/{index}") for index in range(11)]
        rows.extend([("Focus: Duplicate", "/same"), ("Focus: Duplicate", "/same")])
        provider = PaletteWalker(rows)
        result = await provider._walk_rows(None, "0xabc", PaletteState(None, None, None))  # type: ignore[arg-type]
        self.assertEqual(len(result), 13)
        self.assertEqual(result[-2].occurrence, 1)
        self.assertEqual(result[-1].occurrence, 2)
        self.assertEqual(result[-1].ordinal, 12)

    async def test_palette_scan_fails_if_selection_leaves_focus_rows(self) -> None:
        provider = PaletteWalker([("Focus: One", "/one"), ("Close window", "")])
        with self.assertRaises(CommandError):
            await provider._walk_rows(None, "0xabc", PaletteState(None, None, None))  # type: ignore[arg-type]

    async def test_navigation_uses_exact_address_key_state_pairs(self) -> None:
        runner = FakeRunner("ok\n")
        context = ScanContext(runner, ProcTable({}))  # type: ignore[arg-type]
        palette = PaletteState(object(), object(), object())
        with patch.object(GhosttyProvider, "_find_palette", return_value=palette), patch(
            "everything.providers.ghostty._text_value", return_value="Focus:"
        ):
            await GhosttyProvider()._send_verified_key(context, "0xabc", palette, "DOWN")
        self.assertEqual(len(runner.calls), 2)
        expressions = [call[2] for call in runner.calls]
        self.assertTrue(all("hl.dsp.send_key_state" in value for value in expressions))
        self.assertTrue(all('window = "address:0xabc"' in value for value in expressions))
        self.assertTrue(all('key = "DOWN"' in value for value in expressions))
        self.assertIn('state = "down"', expressions[0])
        self.assertIn('state = "up"', expressions[1])
        self.assertTrue(all("sendshortcut" not in value for value in expressions))

    async def test_palette_without_a_native_toggle_never_injects_an_opening_binding(self) -> None:
        top = type("Top", (), {"accessible": object(), "client": {"address": "0xabc"}})()
        with patch("everything.providers.ghostty.walk", return_value=[]):
            with self.assertRaises(PaletteUnavailable):
                GhosttyProvider._command_palette_toggle(top)


@unittest.skipIf(Atspi is None, "PyGObject AT-SPI is unavailable")
class GhosttyLifecycleTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.focus = self.enterContext(patch("everything.providers.ghostty.focus_address", new=AsyncMock()))

    async def test_palette_uses_live_capabilities_without_a_version_command(self):
        ghostty = FakeGhostty()
        context = ghostty.context()
        context.runner.run = AsyncMock(side_effect=AssertionError("no executable probe is needed"))
        async with GhosttyProvider()._palette_session(context, ghostty.top) as palette:
            self.assertIs(palette.search, ghostty.search)
            self.assertEqual(ghostty.search.text, "Focus:")
        context.runner.run.assert_not_awaited()
        self.assertEqual(ghostty.toggles, 2)
        self.assertFalse(ghostty.opened)

    async def test_missing_palette_action_is_rejected_before_focus(self):
        ghostty = FakeGhostty()
        ghostty.frame.action = None
        with self.assertRaisesRegex(PaletteUnavailable, "capability is unavailable"):
            async with GhosttyProvider()._palette_session(ghostty.context(), ghostty.top):
                self.fail("missing palette action was accepted")
        self.assertEqual(ghostty.toggles, 0)
        self.focus.assert_not_awaited()

    async def test_unnamed_deep_search_uses_its_own_modal(self):
        ghostty = FakeGhostty()
        ghostty.toggle()
        palette = GhosttyProvider._find_palette(ghostty.application)
        self.assertIsNotNone(palette)
        self.assertIs(palette.search, ghostty.search)
        self.assertIs(palette.container, ghostty.dialog)
        ghostty.application.children = [ghostty.frame, ghostty.search, ghostty.list_view]
        ghostty.search.parent = ghostty.application
        self.assertIsNone(GhosttyProvider._find_palette(ghostty.application))

    async def test_failed_detection_and_uncommitted_query_close_through_the_opening_action(self):
        for options in ({"editable": False}, {"accept_text": False}):
            with self.subTest(options=options):
                ghostty = FakeGhostty(**options)
                context = ghostty.context()
                provider = GhosttyProvider()
                with self.assertRaises(PaletteUnavailable):
                    await provider._enumerate_palette(context, ghostty.top)
                self.assertFalse(ghostty.opened)
                self.assertEqual(ghostty.toggles, 2)
                self.assertFalse(any("dispatch" in call for call in context.runner.calls))

    async def test_cleanup_survives_owner_cancellation_flag(self):
        import threading

        ghostty = FakeGhostty()
        provider = GhosttyProvider()
        context = ghostty.context()
        cancelled = threading.Event()
        token = _AT_SPI_CANCEL_EVENT.set(cancelled)
        try:
            with self.assertRaises(asyncio.CancelledError):
                async with provider._palette_session(context, ghostty.top):
                    cancelled.set()
                    raise asyncio.CancelledError
        finally:
            _AT_SPI_CANCEL_EVENT.reset(token)
        self.assertFalse(ghostty.opened)
        self.assertEqual(ghostty.toggles, 2)

    async def test_existing_or_already_dismissed_dialog_is_not_toggled(self):
        ghostty = FakeGhostty()
        provider = GhosttyProvider()
        ghostty.toggle()
        with self.assertRaisesRegex(CommandError, "already has an open dialog"):
            async with provider._palette_session(ghostty.context(), ghostty.top):
                self.fail("an existing dialog was reused")
        self.assertEqual(ghostty.toggles, 1)
        ghostty.toggle()
        async with provider._palette_session(ghostty.context(), ghostty.top):
            ghostty.toggle()  # Simulate successful Enter or user dismissal.
        self.assertEqual(ghostty.toggles, 4)
        self.assertFalse(ghostty.opened)

    async def test_first_surface_activation_closes_the_palette_without_reopening_it(self):
        ghostty = FakeGhostty()
        provider = GhosttyProvider()
        context = ghostty.context()
        fingerprint = PaletteFingerprint("Shell", "/work", 1, 0)

        async def key(_context, _address, _palette, value):
            if value == "RETURN":
                ghostty.toggle()

        with patch("everything.providers.ghostty.AtspiTree") as tree, patch.object(
            provider, "_walk_rows", new=AsyncMock(return_value=[fingerprint])
        ), patch.object(provider, "_send_verified_key", side_effect=key), patch.object(
            provider, "_wait_selected", new=AsyncMock(return_value=object())
        ), patch.object(provider, "_row_values", return_value=("Focus: Shell", "/work")):
            tree.return_value.top_levels.return_value = [ghostty.top]
            await provider._activate_surface({
                "pid": 17, "control_address": "0xabc", "host_address": "0xabc",
                "fingerprint": fingerprint.json(),
            }, context)
        self.assertFalse(ghostty.opened)
        self.assertEqual(ghostty.toggles, 2)

    async def test_revoked_permission_blocks_surface_activation(self):
        ghostty = FakeGhostty()
        with self.assertRaisesRegex(PaletteUnavailable, "disabled"):
            async with GhosttyProvider()._palette_session(ghostty.context(allow=False), ghostty.top):
                self.fail("revoked permission opened the palette")
        self.assertEqual(ghostty.toggles, 0)
        self.focus.assert_not_awaited()

    async def test_scan_preserves_native_tabs_and_disables_a_failed_capability_until_process_replacement(self):
        ghostty = FakeGhostty(accept_text=False)
        provider = GhosttyProvider()
        with patch("everything.providers.ghostty.AtspiTree") as tree, patch(
            "everything.providers.ghostty.process_birth", return_value="birth-one"
        ) as birth:
            tree.return_value.top_levels.return_value = [ghostty.top]
            tree.return_value.native_tabs.side_effect = lambda *_args, **_kw: ghostty.tabs
            for _ in range(2):
                result = await provider.scan(ghostty.context())
                self.assertEqual([item.kind for item in result.items], ["terminal-tab"])
                self.assertEqual(result.warnings, [])
                self.assertEqual(ghostty.toggles, 2)
            birth.return_value = "birth-two"
            await provider.scan(ghostty.context())
            self.assertEqual(ghostty.toggles, 4)

    async def test_default_and_poll_scans_refresh_native_tabs_without_probing_or_focusing(self):
        ghostty = FakeGhostty()
        provider = GhosttyProvider()
        with patch("everything.providers.ghostty.AtspiTree") as tree:
            tree.return_value.top_levels.return_value = [ghostty.top]
            tree.return_value.native_tabs.side_effect = lambda *_args, **_kw: ghostty.tabs
            result = await provider.scan(ghostty.context(allow=False))
            self.assertEqual([item.kind for item in result.items], ["terminal-tab"])
            ghostty.tabs = []
            result = await provider.scan(ghostty.context(allow=False, refresh=False))
            self.assertEqual(result.items, [])
            self.assertEqual(provider.native_tabs, {})
        self.assertEqual(ghostty.toggles, 0)
        self.focus.assert_not_awaited()

    async def test_poll_retains_surfaces_until_permission_or_the_controlling_window_is_gone(self):
        ghostty = FakeGhostty()
        provider = GhosttyProvider()
        with patch("everything.providers.ghostty.AtspiTree") as tree, patch.object(
            provider, "_enumerate_palette", new=AsyncMock(return_value=[PaletteFingerprint("Shell", "/work", 1, 0)])
        ) as probe:
            tree.return_value.top_levels.return_value = [ghostty.top]
            tree.return_value.native_tabs.return_value = []
            initial = await provider.scan(ghostty.context())
            self.assertEqual(len(initial.items), 1)
            polled = await provider.scan(ghostty.context(refresh=False))
            self.assertEqual(polled.items, initial.items)
            probe.assert_awaited_once()
            disabled = await provider.scan(ghostty.context(allow=False, refresh=False))
            self.assertEqual(disabled.items, [])
            self.assertEqual(disabled.metadata["surfaces"], [])
            await provider.scan(ghostty.context())
            closed_context = ghostty.context(refresh=False)
            closed_context.hypr_clients = []
            closed = await provider.scan(closed_context)
            self.assertEqual(closed.items, [])
            self.assertEqual(closed.metadata, {})










class HostRoutingTests(unittest.TestCase):
    def test_shared_foot_pid_is_never_guessed(self) -> None:
        processes = {
            10: ProcessInfo(10, 1, "a", "foot", ("foot", "--server"), "/"),
            20: ProcessInfo(20, 10, "b", "bash", ("bash",), "/work"),
        }
        context = ScanContext(FakeRunner(""), ProcTable(processes))  # type: ignore[arg-type]
        context.hypr_clients = [
            {"address": "0xaaa", "pid": 10, "class": "foot"},
            {"address": "0xbbb", "pid": 10, "class": "foot"},
        ]
        self.assertIsNone(route_for_process(context, 20, cwd="/work"))

    def test_single_internal_surface_routes_before_outer_window(self) -> None:
        processes = {
            100: ProcessInfo(100, 1, "a", "ghostty", ("ghostty",), "/work"),
            110: ProcessInfo(110, 100, "b", "bash", ("bash",), "/work"),
        }
        context = ScanContext(FakeRunner(""), ProcTable(processes))  # type: ignore[arg-type]
        context.hypr_clients = [{"address": "0xaaa", "pid": 100, "class": "ghostty"}]
        context.provider_metadata["ghostty"] = {
            "surfaces": [
                {
                    "pid": 100,
                    "item_id": "surface",
                    "activation": {"target": "surface"},
                }
            ]
        }
        route = route_for_process(context, 110, cwd="/work")
        self.assertEqual(route["provider"], "ghostty")  # type: ignore[index]
        self.assertEqual(route["item_id"], "surface")  # type: ignore[index]


class GhosttyWindowRoutingTests(unittest.TestCase):
    def _context(self, clients: list[dict]) -> ScanContext:
        processes = {
            100: ProcessInfo(100, 1, "a", "ghostty", ("ghostty",), "/work"),
            110: ProcessInfo(110, 100, "b", "herdr", ("herdr",), "/work"),
        }
        context = ScanContext(FakeRunner(""), ProcTable(processes))  # type: ignore[arg-type]
        context.hypr_clients = clients
        return context

    def test_ghostty_without_pane_discovery_routes_to_its_only_window(self) -> None:
        context = self._context([{"address": "0xaaa", "pid": 100, "class": "ghostty"}])
        route = route_for_process(context, 110, cwd="/work")
        self.assertEqual(route["provider"], "hyprland")  # type: ignore[index]
        self.assertEqual(route["activation"]["address"], "0xaaa")  # type: ignore[index]

    def test_ghostty_without_pane_discovery_is_not_guessed_across_windows(self) -> None:
        context = self._context([
            {"address": "0xaaa", "pid": 100, "class": "ghostty"},
            {"address": "0xbbb", "pid": 100, "class": "ghostty"},
        ])
        self.assertIsNone(route_for_process(context, 110, cwd="/work"))




class FreshTerminalTests(unittest.IsolatedAsyncioTestCase):
    async def test_new_exact_address_is_detected_and_focused(self) -> None:
        class LaunchRunner:
            def __init__(self) -> None:
                self.client_reads = 0
                self.spawned = None
                self.focus_expression = ""

            async def run(self, argv, **_kwargs):
                command = tuple(str(value) for value in argv)
                if command[:3] == ("hyprctl", "-j", "clients"):
                    self.client_reads += 1
                    clients = [{"address": "0x0dd", "pid": 1, "class": "foot", "mapped": True}]
                    if self.client_reads > 1:
                        clients.append(
                            {"address": "0xbee", "pid": 2, "class": "foot", "mapped": True}
                        )
                    return CommandResult(command, 0, json.dumps(clients), "")
                self.focus_expression = command[2]
                return CommandResult(command, 0, "ok\n", "")

            async def spawn_detached(self, argv, **_kwargs):
                self.spawned = tuple(str(value) for value in argv)
                return 99

        runner = LaunchRunner()
        context = ScanContext(runner, ProcTable({}))  # type: ignore[arg-type]
        address = await launch_terminal_and_focus(
            context, ["omarchy-launch-terminal", "tmux", "attach"]
        )
        self.assertEqual(address, "0xbee")
        self.assertEqual(runner.spawned, ("omarchy-launch-terminal", "tmux", "attach"))
        self.assertIn('address:0xbee', runner.focus_expression)


if __name__ == "__main__":
    unittest.main()
