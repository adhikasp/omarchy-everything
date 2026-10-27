from __future__ import annotations

import asyncio
import json
import os
import stat
from typing import Any
from urllib.parse import urlsplit

from ..commands import CommandError
from ..model import Thing, stable_id
from .base import ProviderResult, ScanContext
from .hyprland import _live_clients, focus_address


def socket_path() -> str:
    runtime = os.environ.get("XDG_RUNTIME_DIR") or "/tmp"
    return os.environ.get("SCREEN_CONTEXT_SOCK", os.path.join(runtime, "screen-context", "host.sock"))


def extension_available() -> bool:
    """True when the Screen Context host socket exists, so Chrome tabs come from the extension."""

    try:
        return stat.S_ISSOCK(os.stat(socket_path()).st_mode)
    except OSError:
        return False


def is_chrome_client(client: dict[str, Any]) -> bool:
    # Web apps use "chrome-<host>__-Default"; only real browser windows hold tabs.
    cls = str(client.get("class") or "").lower()
    return cls.startswith(("google-chrome", "chromium"))


async def host_request(request: dict[str, Any], timeout: float = 2.0) -> dict[str, Any]:
    """One JSON request to the Screen Context native host (Chrome extension bridge)."""

    reader, writer = await asyncio.wait_for(asyncio.open_unix_connection(socket_path()), timeout)
    try:
        writer.write((json.dumps(request) + "\n").encode())
        await writer.drain()
        line = await asyncio.wait_for(reader.readline(), timeout)
    finally:
        writer.close()
    value = json.loads(line)
    if not isinstance(value, dict):
        raise CommandError("Chrome extension returned malformed data")
    return value


class ChromeTabsProvider:
    """Chrome tabs through the Screen Context extension, since Chrome exposes
    no tab strip over AT-SPI on this stack. Silent when the extension is absent."""

    name = "chrome"

    async def scan(self, context: ScanContext) -> ProviderResult:
        if not any(is_chrome_client(client) for client in context.hypr_clients):
            return ProviderResult(provider=self.name)
        try:
            response = await host_request({"op": "tabs"})
        except (FileNotFoundError, ConnectionRefusedError, asyncio.TimeoutError):
            return ProviderResult(provider=self.name)
        if response.get("ok") is not True:
            return ProviderResult(provider=self.name, warnings=[f"Chrome tabs: {response.get('error') or 'failed'}"])

        items: list[Thing] = []
        for tab in response.get("result") or []:
            tab_id = tab.get("id")
            if not isinstance(tab_id, int) or tab.get("windowType", "normal") != "normal":
                continue
            url = str(tab.get("url") or "")
            host = urlsplit(url).hostname or ""
            badges = ["Tab"]
            if tab.get("pinned"):
                badges.append("Pinned")
            if tab.get("audible"):
                badges.append("Audible")
            items.append(
                Thing(
                    id=stable_id("chrome", tab_id),
                    kind="browser-tab",
                    provider="Chrome",
                    title=str(tab.get("title") or url or "New Tab"),
                    icon_hints=["google-chrome"],
                    context=host,
                    search_terms=["chrome", url],
                    badges=badges,
                    active=bool(tab.get("active")),
                    activation={"tab_id": tab_id, "title": str(tab.get("title") or ""), "url": url},
                )
            )
        return ProviderResult(provider=self.name, items=items)

    async def activate(self, activation: dict[str, Any], context: ScanContext) -> None:
        tab_id = activation.get("tab_id")
        if not isinstance(tab_id, int):
            raise CommandError("Chrome tab is no longer available")
        try:
            response = await host_request({"op": "activate_tab", "tabId": tab_id})
        except (FileNotFoundError, ConnectionRefusedError, asyncio.TimeoutError) as error:
            raise CommandError("Chrome extension is not reachable") from error
        if response.get("ok") is not True:
            raise CommandError("That tab is no longer available")

        # Selecting the tab makes its window title "<tab title> - Google Chrome";
        # find that exact client and raise it through Hyprland (works across workspaces).
        wanted = str(response.get("result", {}).get("title") or activation.get("title") or "")
        for _ in range(8):
            clients = [c for c in await _live_clients(context) if is_chrome_client(c) and c.get("mapped")]
            matches = [c for c in clients if wanted and str(c.get("title") or "").startswith(wanted)]
            if len(matches) == 1 or (matches and len(clients) > 1):
                await focus_address(context, str(matches[0].get("address")))
                return
            if not wanted and len(clients) == 1:
                await focus_address(context, str(clients[0].get("address")))
                return
            await asyncio.sleep(0.15)
        raise CommandError("Could not find the Chrome window for that tab")
