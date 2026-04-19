"""Base class for plugin-defined widgets, plus content-tree builders."""


class WidgetContext:
    """Methods widget callbacks can use."""

    def __init__(self, plugin, pane_id: str):
        self._plugin = plugin
        self.pane_id = pane_id

    def widget_set(self, content):
        self._plugin._send_widget_set(self.pane_id, content)

    def heading(self, text): return {"heading": text}
    def label(self, text): return {"label": text}
    def big(self, text, small=None):
        node = {"big": text}
        if small: node["small"] = small
        return node
    def spacer(self): return {"spacer": True}
    def badge(self, text): return {"badge": text}
    def vbox(self, *children):
        return {"layout": "vertical", "children": list(children)}
    def hbox(self, *children):
        return {"layout": "horizontal", "children": list(children)}


class Widget:
    """Base class. Override `on_will_appear`, `render`, `on_will_disappear`."""

    def on_will_appear(self, ctx: WidgetContext): pass
    def on_will_disappear(self, ctx: WidgetContext): pass
    def render(self, ctx: WidgetContext):
        """Return a content-tree dict (or None)."""
        return None
