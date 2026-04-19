"""View-tree builders for the locked `widget.view_update` wire format.

Each builder returns a plain dict matching the wire shape. Optional fields
whose value is ``None`` are stripped so the resulting NDJSON stays small.

Wire shape (excerpt)::

    {"kind": "vstack", "gap": 4, "align": "start", "children": [ ... ]}
    {"kind": "hstack", "gap": 4, "align": "start", "children": [ ... ]}
    {"kind": "text",   "value": "...", "size": 18, "color": "#cdd6f4", "weight": "bold"}
    {"kind": "icon",   "emoji": "..."}            # OR "path": "/abs/path.png"
    {"kind": "bar",    "value": 0.42, "color": "#a6e3a1", "label": "speaking"}
    {"kind": "spacer", "size": 8}
    {"kind": "divider"}
    {"kind": "image",  "url": "https://...", "width": 64, "height": 64, "fit": "contain"}
    {"kind": "button", "label": "Go", "action": "deck.page_goto", "args": {...}}
    {"kind": "plot",   "values": [1.0, 2.0, ...], "color": "blue", "height": 40}
    {"kind": "table",  "headers": [...], "rows": [[...], ...]}
    {"kind": "scroll", "child": {...}, "height": 200}
    {"kind": "padding","child": {...}, "all": 8}
    {"kind": "bg",     "child": {...}, "color": "surface0", "rounding": 6}
    {"kind": "progress","value": 42, "max": 100, "label": "CPU", "show_percent": true}
    {"kind": "kpi",    "value": "1,234", "label": "users", "delta": "+42", "delta_positive": true}
"""

from typing import Any, Literal, Optional

Align = Literal["start", "center", "end"]
ImageFit = Literal["contain", "cover", "fill"]


def _strip_none(d: dict) -> dict:
    """Return a new dict with keys whose value is ``None`` removed."""
    return {k: v for k, v in d.items() if v is not None}


def vstack(
    *children: dict,
    gap: float = 4.0,
    align: Align = "start",
) -> dict:
    """A vertical stack of child nodes."""
    return {
        "kind": "vstack",
        "gap": gap,
        "align": align,
        "children": list(children),
    }


def hstack(
    *children: dict,
    gap: float = 4.0,
    align: Align = "start",
) -> dict:
    """A horizontal stack of child nodes."""
    return {
        "kind": "hstack",
        "gap": gap,
        "align": align,
        "children": list(children),
    }


def text(
    value: str,
    *,
    size: Optional[float] = None,
    color: Optional[str] = None,
    weight: Optional[str] = None,
) -> dict:
    """A text node. ``size``, ``color``, ``weight`` are optional and stripped if None."""
    return _strip_none(
        {
            "kind": "text",
            "value": value,
            "size": size,
            "color": color,
            "weight": weight,
        }
    )


def icon_emoji(emoji: str, *, size: Optional[float] = None) -> dict:
    """An icon rendered from an emoji glyph."""
    return _strip_none({"kind": "icon", "emoji": emoji, "size": size})


def icon_path(path: str, *, size: Optional[float] = None) -> dict:
    """An icon loaded from an absolute filesystem path."""
    return _strip_none({"kind": "icon", "path": path, "size": size})


def bar(
    value: float,
    *,
    color: Optional[str] = None,
    label: Optional[str] = None,
) -> dict:
    """A horizontal progress/level bar with value in ``[0.0, 1.0]``."""
    return _strip_none(
        {
            "kind": "bar",
            "value": value,
            "color": color,
            "label": label,
        }
    )


def spacer(size: float = 4.0) -> dict:
    """A blank gap of the given size (along the parent stack's axis)."""
    return {"kind": "spacer", "size": size}


def divider() -> dict:
    """A thin separator line."""
    return {"kind": "divider"}


def image(
    *,
    url: Optional[str] = None,
    path: Optional[str] = None,
    data_url: Optional[str] = None,
    width: Optional[float] = None,
    height: Optional[float] = None,
    fit: Optional[ImageFit] = None,
) -> dict:
    """An image loaded from exactly one of ``url``, ``path``, or ``data_url``."""
    sources = [s for s in (url, path, data_url) if s is not None]
    if len(sources) != 1:
        raise ValueError("image() requires exactly one of url/path/data_url")
    body: dict[str, Any] = {"kind": "image"}
    if url is not None:
        body["url"] = url
    if path is not None:
        body["path"] = path
    if data_url is not None:
        body["data_url"] = data_url
    body["width"] = width
    body["height"] = height
    body["fit"] = fit
    return _strip_none(body)


def button(
    label: str,
    action: str,
    args: Optional[dict] = None,
    *,
    color: Optional[str] = None,
) -> dict:
    """A clickable button that publishes ``widget.action_request`` on click."""
    return _strip_none(
        {
            "kind": "button",
            "label": label,
            "action": action,
            "args": args,
            "color": color,
        }
    )


def plot(
    values: list[float],
    *,
    color: Optional[str] = None,
    height: Optional[float] = None,
    label: Optional[str] = None,
) -> dict:
    """A sparkline of f32 values."""
    return _strip_none(
        {
            "kind": "plot",
            "values": list(values),
            "color": color,
            "height": height,
            "label": label,
        }
    )


def table(
    headers: list[str],
    rows: list[list[str]],
    *,
    header_color: Optional[str] = None,
) -> dict:
    """A header row + body rows of strings."""
    return _strip_none(
        {
            "kind": "table",
            "headers": list(headers),
            "rows": [list(r) for r in rows],
            "header_color": header_color,
        }
    )


def scroll(child: dict, *, height: Optional[float] = None) -> dict:
    """Wrap ``child`` in a vertically scrollable area."""
    return _strip_none({"kind": "scroll", "child": child, "height": height})


def padding(
    child: dict,
    *,
    all: Optional[float] = None,
    top: Optional[float] = None,
    right: Optional[float] = None,
    bottom: Optional[float] = None,
    left: Optional[float] = None,
) -> dict:
    """Wrap ``child`` with padding. ``all`` sets every side; per-side values override."""
    return _strip_none(
        {
            "kind": "padding",
            "child": child,
            "all": all,
            "top": top,
            "right": right,
            "bottom": bottom,
            "left": left,
        }
    )


def bg(child: dict, color: str, *, rounding: Optional[float] = None) -> dict:
    """Wrap ``child`` with a background fill + optional rounded corners."""
    return _strip_none(
        {
            "kind": "bg",
            "child": child,
            "color": color,
            "rounding": rounding,
        }
    )


def progress(
    value: float,
    *,
    max: Optional[float] = None,
    color: Optional[str] = None,
    label: Optional[str] = None,
    show_percent: Optional[bool] = None,
) -> dict:
    """A labelled bar with optional percentage overlay."""
    return _strip_none(
        {
            "kind": "progress",
            "value": value,
            "max": max,
            "color": color,
            "label": label,
            "show_percent": show_percent,
        }
    )


def kpi(
    value: str,
    *,
    label: Optional[str] = None,
    delta: Optional[str] = None,
    delta_positive: Optional[bool] = None,
    color: Optional[str] = None,
) -> dict:
    """A big number with label + optional delta indicator."""
    return _strip_none(
        {
            "kind": "kpi",
            "value": value,
            "label": label,
            "delta": delta,
            "delta_positive": delta_positive,
            "color": color,
        }
    )


def kpi_card(
    label: str,
    value: str,
    *,
    delta: Optional[str] = None,
    color: Optional[str] = None,
) -> dict:
    """A padded, rounded card containing a `kpi` node. Handy for dashboards."""
    inner = kpi(value, label=label, delta=delta, color=color)
    return bg(padding(inner, all=8), color="surface0", rounding=6)


def status_grid(
    items: list[tuple[str, str, str]],
    *,
    columns: int = 2,
) -> dict:
    """A grid of (label, value, color) cells laid out as rows of hstacks.

    ``columns`` cells are packed per row. Each cell renders as a vstack with the
    label small + value in the given color.
    """
    if columns < 1:
        raise ValueError("columns must be >= 1")
    cells: list[dict] = [
        vstack(
            text(label, size=11, color="subtext0"),
            text(value, size=16, weight="bold", color=color),
            gap=2,
        )
        for (label, value, color) in items
    ]
    rows = [cells[i : i + columns] for i in range(0, len(cells), columns)]
    return vstack(*[hstack(*row, gap=12) for row in rows], gap=8)


def sparkline(
    values: list[float],
    *,
    label: Optional[str] = None,
    color: Optional[str] = None,
) -> dict:
    """A KPI showing the current value + a plot of the trend underneath."""
    current = f"{values[-1]:g}" if values else "-"
    return vstack(
        kpi(current, label=label, color=color),
        plot(values, color=color, height=30),
        gap=4,
    )


def view_update_message(pane: str, tree: dict) -> dict[str, Any]:
    """Build the full ``widget.view_update`` envelope as a dict."""
    return {"kind": "widget.view_update", "pane": pane, "tree": tree}
