"""Tests for view-tree builders and Plugin.push_view NDJSON output."""

import asyncio
import json
from unittest.mock import MagicMock

import pytest

from juballer_deck import Plugin
from juballer_deck.view import (
    bar,
    bg,
    button,
    divider,
    hstack,
    icon_emoji,
    icon_path,
    image,
    kpi,
    kpi_card,
    padding,
    plot,
    progress,
    scroll,
    spacer,
    sparkline,
    status_grid,
    table,
    text,
    view_update_message,
    vstack,
)


# ---- builder shape tests ----


def test_text_minimal():
    assert text("hi") == {"kind": "text", "value": "hi"}


def test_text_full():
    assert text("hi", size=18, color="#cdd6f4", weight="bold") == {
        "kind": "text",
        "value": "hi",
        "size": 18,
        "color": "#cdd6f4",
        "weight": "bold",
    }


def test_text_strips_none():
    # Only `weight` provided — size/color must be absent, not present-and-null.
    out = text("hi", weight="bold")
    assert out == {"kind": "text", "value": "hi", "weight": "bold"}
    assert "size" not in out
    assert "color" not in out


def test_icon_emoji_minimal():
    assert icon_emoji("🎤") == {"kind": "icon", "emoji": "🎤"}


def test_icon_emoji_with_size():
    assert icon_emoji("🎤", size=24) == {"kind": "icon", "emoji": "🎤", "size": 24}


def test_icon_path():
    assert icon_path("/abs/path.png", size=12) == {
        "kind": "icon",
        "path": "/abs/path.png",
        "size": 12,
    }


def test_icon_path_strips_none():
    assert icon_path("/abs/path.png") == {"kind": "icon", "path": "/abs/path.png"}


def test_bar_minimal():
    assert bar(0.5) == {"kind": "bar", "value": 0.5}


def test_bar_full():
    assert bar(0.42, color="#a6e3a1", label="speaking") == {
        "kind": "bar",
        "value": 0.42,
        "color": "#a6e3a1",
        "label": "speaking",
    }


def test_spacer_default():
    assert spacer() == {"kind": "spacer", "size": 4.0}


def test_spacer_custom():
    assert spacer(8) == {"kind": "spacer", "size": 8}


def test_divider():
    assert divider() == {"kind": "divider"}


def test_vstack_defaults():
    out = vstack(text("a"), text("b"))
    assert out == {
        "kind": "vstack",
        "gap": 4.0,
        "align": "start",
        "children": [
            {"kind": "text", "value": "a"},
            {"kind": "text", "value": "b"},
        ],
    }


def test_hstack_custom_gap_align():
    out = hstack(text("a"), gap=6, align="center")
    assert out == {
        "kind": "hstack",
        "gap": 6,
        "align": "center",
        "children": [{"kind": "text", "value": "a"}],
    }


def test_nested_compose():
    """vstack containing hstack containing leaves — must round-trip through JSON unchanged."""
    tree = vstack(
        text("Header", size=18, weight="bold"),
        hstack(icon_emoji("🟢", size=12), text("user1", size=14), gap=6),
        divider(),
        bar(0.42, color="#a6e3a1", label="speaking"),
        spacer(8),
    )
    encoded = json.dumps(tree)
    decoded = json.loads(encoded)
    assert decoded == tree
    assert decoded["kind"] == "vstack"
    assert decoded["children"][1]["kind"] == "hstack"
    assert decoded["children"][1]["children"][0]["emoji"] == "🟢"
    assert decoded["children"][2] == {"kind": "divider"}


def test_view_update_message_envelope():
    tree = vstack(text("hi"))
    env = view_update_message("discord", tree)
    assert env == {"kind": "widget.view_update", "pane": "discord", "tree": tree}


# ---- new primitive builders ----


def test_image_url_minimal():
    assert image(url="https://x/y.png") == {"kind": "image", "url": "https://x/y.png"}


def test_image_path_full():
    assert image(path="/a.png", width=64, height=64, fit="contain") == {
        "kind": "image",
        "path": "/a.png",
        "width": 64,
        "height": 64,
        "fit": "contain",
    }


def test_image_data_url():
    d = "data:image/png;base64,AAAA"
    assert image(data_url=d) == {"kind": "image", "data_url": d}


def test_image_requires_exactly_one_source():
    with pytest.raises(ValueError):
        image()
    with pytest.raises(ValueError):
        image(url="a", path="b")


def test_image_strips_none():
    out = image(url="u")
    assert "width" not in out
    assert "height" not in out
    assert "fit" not in out


def test_button_minimal():
    assert button("Go", "deck.page_goto") == {
        "kind": "button",
        "label": "Go",
        "action": "deck.page_goto",
    }


def test_button_full():
    assert button("Mute", "discord.mute", {"toggle": True}, color="green") == {
        "kind": "button",
        "label": "Mute",
        "action": "discord.mute",
        "args": {"toggle": True},
        "color": "green",
    }


def test_plot_minimal():
    assert plot([1.0, 2.0, 3.0]) == {"kind": "plot", "values": [1.0, 2.0, 3.0]}


def test_plot_full():
    assert plot([1.0, 2.0], color="blue", height=40, label="cpu") == {
        "kind": "plot",
        "values": [1.0, 2.0],
        "color": "blue",
        "height": 40,
        "label": "cpu",
    }


def test_table_minimal():
    assert table(["a", "b"], [["1", "2"], ["3", "4"]]) == {
        "kind": "table",
        "headers": ["a", "b"],
        "rows": [["1", "2"], ["3", "4"]],
    }


def test_table_with_header_color():
    out = table(["a"], [["x"]], header_color="mauve")
    assert out["header_color"] == "mauve"


def test_scroll_minimal():
    child = text("x")
    assert scroll(child) == {"kind": "scroll", "child": child}


def test_scroll_with_height():
    child = text("x")
    assert scroll(child, height=200) == {"kind": "scroll", "child": child, "height": 200}


def test_padding_all():
    child = divider()
    assert padding(child, all=8) == {"kind": "padding", "child": child, "all": 8}


def test_padding_sides():
    child = divider()
    out = padding(child, top=4, right=8, bottom=4, left=8)
    assert out == {
        "kind": "padding",
        "child": child,
        "top": 4,
        "right": 8,
        "bottom": 4,
        "left": 8,
    }
    assert "all" not in out


def test_bg_minimal():
    child = divider()
    assert bg(child, "surface0") == {
        "kind": "bg",
        "child": child,
        "color": "surface0",
    }


def test_bg_rounded():
    child = divider()
    assert bg(child, "#1e1e2e", rounding=6) == {
        "kind": "bg",
        "child": child,
        "color": "#1e1e2e",
        "rounding": 6,
    }


def test_progress_minimal():
    assert progress(0.5) == {"kind": "progress", "value": 0.5}


def test_progress_full():
    assert progress(42, max=100, color="green", label="CPU", show_percent=True) == {
        "kind": "progress",
        "value": 42,
        "max": 100,
        "color": "green",
        "label": "CPU",
        "show_percent": True,
    }


def test_kpi_minimal():
    assert kpi("99") == {"kind": "kpi", "value": "99"}


def test_kpi_full():
    assert kpi(
        "1,234",
        label="users",
        delta="+42",
        delta_positive=True,
        color="mauve",
    ) == {
        "kind": "kpi",
        "value": "1,234",
        "label": "users",
        "delta": "+42",
        "delta_positive": True,
        "color": "mauve",
    }


# ---- high-level convenience helpers ----


def test_kpi_card_wraps_bg_padding_kpi():
    out = kpi_card("users", "123", delta="+5", color="green")
    assert out["kind"] == "bg"
    assert out["color"] == "surface0"
    assert out["rounding"] == 6
    pad = out["child"]
    assert pad["kind"] == "padding"
    assert pad["all"] == 8
    inner = pad["child"]
    assert inner["kind"] == "kpi"
    assert inner["value"] == "123"
    assert inner["label"] == "users"
    assert inner["delta"] == "+5"
    assert inner["color"] == "green"


def test_status_grid_shape():
    out = status_grid(
        [
            ("cpu", "42%", "green"),
            ("mem", "81%", "peach"),
            ("disk", "50%", "blue"),
            ("net", "ok", "sky"),
        ],
        columns=2,
    )
    # outer vstack of 2 hstacks of 2 cells each
    assert out["kind"] == "vstack"
    rows = out["children"]
    assert len(rows) == 2
    assert all(r["kind"] == "hstack" for r in rows)
    assert len(rows[0]["children"]) == 2
    cell = rows[0]["children"][0]
    assert cell["kind"] == "vstack"
    assert cell["children"][0]["value"] == "cpu"
    assert cell["children"][1]["value"] == "42%"
    assert cell["children"][1]["color"] == "green"


def test_status_grid_partial_last_row():
    out = status_grid(
        [("a", "1", "blue"), ("b", "2", "blue"), ("c", "3", "blue")], columns=2
    )
    rows = out["children"]
    assert len(rows) == 2
    assert len(rows[0]["children"]) == 2
    assert len(rows[1]["children"]) == 1


def test_status_grid_bad_columns():
    with pytest.raises(ValueError):
        status_grid([("a", "1", "blue")], columns=0)


def test_sparkline_shape():
    out = sparkline([1.0, 2.0, 3.0], label="cpu", color="blue")
    assert out["kind"] == "vstack"
    kids = out["children"]
    assert kids[0]["kind"] == "kpi"
    assert kids[0]["value"] == "3"
    assert kids[0]["label"] == "cpu"
    assert kids[1]["kind"] == "plot"
    assert kids[1]["values"] == [1.0, 2.0, 3.0]


def test_sparkline_empty():
    out = sparkline([])
    assert out["children"][0]["value"] == "-"


def test_every_new_builder_is_json_serializable():
    """All new primitives must round-trip through JSON."""
    trees = [
        image(url="https://x/y.png", width=64),
        button("Go", "deck.page_goto", {"page": "home"}),
        plot([1.0, 2.0, 3.0], color="blue"),
        table(["a", "b"], [["1", "2"]]),
        scroll(text("x"), height=200),
        padding(divider(), all=8),
        bg(divider(), "surface0", rounding=6),
        progress(42, max=100, label="CPU"),
        kpi("99", label="users", delta="+5"),
        kpi_card("users", "123", delta="+5"),
        status_grid([("a", "1", "blue")], columns=1),
        sparkline([1.0, 2.0, 3.0], label="cpu"),
    ]
    for t in trees:
        assert json.loads(json.dumps(t)) == t


# ---- Plugin.push_view writer test ----


class _FakeWriter:
    """Minimal stand-in for asyncio.StreamWriter."""

    def __init__(self):
        self.buf = bytearray()
        self.drains = 0

    def write(self, data: bytes) -> None:
        self.buf.extend(data)

    async def drain(self) -> None:
        self.drains += 1


def test_push_view_writes_ndjson_line():
    plugin = Plugin("test")
    fake = _FakeWriter()
    plugin._writer = fake  # inject

    tree = vstack(text("hello"))
    asyncio.run(plugin.push_view("discord", tree))

    raw = bytes(fake.buf).decode("utf-8")
    assert raw.endswith("\n"), "NDJSON line must end with newline"
    msg = json.loads(raw.rstrip("\n"))
    assert msg == {
        "kind": "widget.view_update",
        "pane": "discord",
        "tree": tree,
    }
    # Spec: no `type` field (this uses the locked `kind` discriminator instead).
    assert "type" not in msg
    assert fake.drains == 1


def test_push_view_with_no_writer_is_noop():
    """Before connect, _writer is None — push_view must not raise."""
    plugin = Plugin("test")
    assert plugin._writer is None
    asyncio.run(plugin.push_view("discord", vstack(text("x"))))  # must not raise


def test_set_named_tile_writes_ndjson_line():
    plugin = Plugin("test")
    fake = _FakeWriter()
    plugin._writer = fake

    asyncio.run(plugin.set_named_tile(
        "discord_unread", icon="💬", label="3 DM", state_color="red",
    ))

    raw = bytes(fake.buf).decode("utf-8")
    msg = json.loads(raw.rstrip("\n"))
    assert msg == {
        "type": "tile_set_by_name",
        "name": "discord_unread",
        "icon": "💬",
        "label": "3 DM",
        "state_color": "red",
    }


def test_set_named_tile_clear_strips_other_fields():
    plugin = Plugin("test")
    fake = _FakeWriter()
    plugin._writer = fake

    asyncio.run(plugin.set_named_tile("discord_unread", clear=True))

    msg = json.loads(bytes(fake.buf).decode("utf-8").rstrip("\n"))
    assert msg == {"type": "tile_set_by_name", "name": "discord_unread", "clear": True}


def test_set_named_tile_omits_falsy_clear():
    """clear=False must be omitted (matches the Rust `Option<bool>` default)."""
    plugin = Plugin("test")
    fake = _FakeWriter()
    plugin._writer = fake

    asyncio.run(plugin.set_named_tile("t", label="hi"))
    msg = json.loads(bytes(fake.buf).decode("utf-8").rstrip("\n"))
    assert "clear" not in msg
