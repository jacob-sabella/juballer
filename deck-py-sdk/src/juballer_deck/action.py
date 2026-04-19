"""Base class for plugin-defined actions."""


class ActionContext:
    """Methods action callbacks can use to push state to the deck."""

    def __init__(self, plugin, binding_id: str):
        self._plugin = plugin
        self.binding_id = binding_id

    def tile_set(self, *, icon: str | None = None, label: str | None = None,
                 state_color: str | None = None):
        self._plugin._send_tile_set(self.binding_id, icon, label, state_color)

    def tile_flash(self, ms: int = 120):
        self._plugin._send_tile_flash(self.binding_id, ms)

    def bus_publish(self, topic: str, data):
        self._plugin._send_bus_publish(topic, data)


class Action:
    """Base class. Override `on_will_appear`, `on_down`, `on_up`, `on_will_disappear`."""

    def on_will_appear(self, ctx: ActionContext): pass
    def on_down(self, ctx: ActionContext): pass
    def on_up(self, ctx: ActionContext): pass
    def on_will_disappear(self, ctx: ActionContext): pass
