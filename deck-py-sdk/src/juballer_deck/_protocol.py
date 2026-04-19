"""NDJSON wire-format types matching juballer-deck-protocol."""

from typing import Any, Optional

PROTOCOL_VERSION = 1


def hello(deck_version: Optional[str] = None, plugin: Optional[str] = None,
          plugin_version: Optional[str] = None, sdk: Optional[str] = None) -> dict:
    msg = {"type": "hello", "v": PROTOCOL_VERSION}
    if deck_version: msg["deck_version"] = deck_version
    if plugin: msg["plugin"] = plugin
    if plugin_version: msg["plugin_version"] = plugin_version
    if sdk: msg["sdk"] = sdk
    return msg


def pong() -> dict:
    return {"type": "pong"}


def tile_set(binding_id: str, icon: Optional[str] = None,
             label: Optional[str] = None, state_color: Optional[str] = None) -> dict:
    msg = {"type": "tile_set", "binding_id": binding_id}
    if icon is not None: msg["icon"] = icon
    if label is not None: msg["label"] = label
    if state_color is not None: msg["state_color"] = state_color
    return msg


def tile_flash(binding_id: str, ms: int = 120) -> dict:
    return {"type": "tile_flash", "binding_id": binding_id, "ms": ms}


def tile_set_by_name(name: str, icon: Optional[str] = None,
                     label: Optional[str] = None, state_color: Optional[str] = None,
                     clear: bool = False) -> dict:
    msg: dict = {"type": "tile_set_by_name", "name": name}
    if icon is not None: msg["icon"] = icon
    if label is not None: msg["label"] = label
    if state_color is not None: msg["state_color"] = state_color
    if clear: msg["clear"] = True
    return msg


def widget_set(pane_id: str, content: Any) -> dict:
    return {"type": "widget_set", "pane_id": pane_id, "content": content}


def bus_publish(topic: str, data: Any) -> dict:
    return {"type": "bus_publish", "topic": topic, "data": data}


def bus_subscribe(topics: list) -> dict:
    return {"type": "bus_subscribe", "topics": topics}


def log(level: str, msg: str) -> dict:
    return {"type": "log", "level": level, "msg": msg}


def error(code: str, msg: str) -> dict:
    return {"type": "error", "code": code, "msg": msg}
