from .atspi import AtspiProvider
from .chrome_tabs import ChromeTabsProvider
from .ghostty import GhosttyProvider
from .herdr import HerdrProvider
from .hyprland import HyprlandProvider
from .kitty import KittyProvider
from .neovim import NeovimProvider
from .tmux import TmuxProvider

__all__ = [
    "AtspiProvider",
    "ChromeTabsProvider",
    "GhosttyProvider",
    "HerdrProvider",
    "HyprlandProvider",
    "KittyProvider",
    "NeovimProvider",
    "TmuxProvider",
]

