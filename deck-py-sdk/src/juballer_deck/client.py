"""Plugin runner — connects to deck via UDS, dispatches messages to user code."""

import asyncio
import json
import os
import sys
from typing import Optional

from . import _protocol
from .action import Action, ActionContext
from .widget import Widget, WidgetContext


class Plugin:
    def __init__(self, name: str, version: str = "0.1.0"):
        self.name = name
        self.version = version
        self._action_classes: dict[str, type[Action]] = {}
        self._widget_classes: dict[str, type[Widget]] = {}
        self._action_instances: dict[str, Action] = {}
        self._widget_instances: dict[str, Widget] = {}
        self._writer: Optional[asyncio.StreamWriter] = None
        self._lock = asyncio.Lock()
        self._pending: list[bytes] = []
        self._connected = asyncio.Event()

    # ---- decorators ----

    def action(self, name: str):
        def deco(cls):
            self._action_classes[name] = cls
            return cls
        return deco

    def widget(self, name: str):
        def deco(cls):
            self._widget_classes[name] = cls
            return cls
        return deco

    # ---- public run ----

    def run(self):
        sock_path = os.environ.get("JUBALLER_SOCK")
        if not sock_path:
            print("juballer-deck plugin: JUBALLER_SOCK env var not set; not connected to a deck.",
                  file=sys.stderr)
            sys.exit(1)
        asyncio.run(self._run(sock_path))

    # ---- internals ----

    async def _send(self, msg: dict):
        line = (json.dumps(msg) + "\n").encode("utf-8")
        async with self._lock:
            if self._writer is None:
                self._pending.append(line)
                return
            self._writer.write(line)
            await self._writer.drain()

    def _send_tile_set(self, binding_id, icon, label, state_color):
        asyncio.create_task(self._send(_protocol.tile_set(binding_id, icon, label, state_color)))

    def _send_tile_flash(self, binding_id, ms):
        asyncio.create_task(self._send(_protocol.tile_flash(binding_id, ms)))

    def _send_widget_set(self, pane_id, content):
        asyncio.create_task(self._send(_protocol.widget_set(pane_id, content)))

    def _send_bus_publish(self, topic, data):
        asyncio.create_task(self._send(_protocol.bus_publish(topic, data)))

    async def set_named_tile(
        self,
        name: str,
        *,
        icon: Optional[str] = None,
        label: Optional[str] = None,
        state_color: Optional[str] = None,
        clear: bool = False,
    ) -> None:
        """Update a tile identified by ``ButtonCfg.name``.

        Omitted fields preserve the tile's current plugin override.
        Pass ``clear=True`` to drop any plugin override and restore the
        config-default icon/label/state_color. ``state_color`` accepts
        ``"#rrggbb[aa]"`` or a Catppuccin token like ``"red"`` / ``"green"``.
        """
        await self._send(
            _protocol.tile_set_by_name(
                name, icon=icon, label=label, state_color=state_color, clear=clear
            )
        )

    async def push_view(self, pane: str, tree: dict) -> None:
        """Send a `widget.view_update` NDJSON line to the deck for the named pane.

        This uses the locked view-tree wire format (see `juballer_deck.view`)
        and is independent of the `type`-tagged Message enum used elsewhere.
        """
        await self._send({"kind": "widget.view_update", "pane": pane, "tree": tree})

    async def _run(self, sock_path: str):
        reader, writer = await asyncio.open_unix_connection(sock_path)
        async with self._lock:
            self._writer = writer
            for line in self._pending:
                writer.write(line)
            self._pending.clear()
            await writer.drain()
        self._connected.set()
        await self._send(_protocol.hello(plugin=self.name, plugin_version=self.version,
                                         sdk="py-0.1.0"))
        while True:
            line = await reader.readline()
            if not line:
                break
            try:
                msg = json.loads(line.decode("utf-8"))
            except json.JSONDecodeError:
                continue
            await self._handle(msg)
        os._exit(0)

    async def _handle(self, msg: dict):
        t = msg.get("type")
        if t == "hello":
            return
        if t == "register_complete":
            return
        if t == "ping":
            await self._send(_protocol.pong())
            return
        if t == "will_appear":
            name = msg["action"]
            binding_id = msg["binding_id"]
            cls = self._action_classes.get(name)
            if cls is None: return
            inst = cls()
            self._action_instances[binding_id] = inst
            inst.on_will_appear(ActionContext(self, binding_id))
            return
        if t == "will_disappear":
            binding_id = msg["binding_id"]
            inst = self._action_instances.pop(binding_id, None)
            if inst:
                inst.on_will_disappear(ActionContext(self, binding_id))
            return
        if t == "key_down":
            binding_id = msg["binding_id"]
            inst = self._action_instances.get(binding_id)
            if inst: inst.on_down(ActionContext(self, binding_id))
            return
        if t == "key_up":
            binding_id = msg["binding_id"]
            inst = self._action_instances.get(binding_id)
            if inst: inst.on_up(ActionContext(self, binding_id))
            return
        if t == "widget_will_appear":
            name = msg["widget"]
            pane_id = msg["pane_id"]
            cls = self._widget_classes.get(name)
            if cls is None: return
            inst = cls()
            self._widget_instances[pane_id] = inst
            inst.on_will_appear(WidgetContext(self, pane_id))
            content = inst.render(WidgetContext(self, pane_id))
            if content is not None:
                self._send_widget_set(pane_id, content)
            return
        if t == "widget_will_disappear":
            pane_id = msg["pane_id"]
            inst = self._widget_instances.pop(pane_id, None)
            if inst:
                inst.on_will_disappear(WidgetContext(self, pane_id))
            return


def cli_main():
    """juballer-plugin run <path/to/plugin.py> — quick dev wrapper."""
    import importlib.util
    if len(sys.argv) < 3 or sys.argv[1] != "run":
        print("usage: juballer-plugin run <path/to/plugin.py>", file=sys.stderr)
        sys.exit(2)
    path = sys.argv[2]
    spec = importlib.util.spec_from_file_location("user_plugin", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    # the user's plugin.py should call plugin.run() in __main__; if not, find a Plugin instance.
    for v in vars(mod).values():
        if isinstance(v, Plugin):
            v.run()
            return
    print("juballer-plugin: no Plugin instance found in module", file=sys.stderr)
    sys.exit(2)
