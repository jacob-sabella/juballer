# juballer-deck (Python SDK)

Python SDK for writing juballer-deck plugins.

## Quick start

```python
from juballer_deck import Plugin, Action, Widget

plugin = Plugin("hello")

@plugin.action("hello.shout")
class ShoutAction(Action):
    def on_down(self, ctx):
        ctx.tile_set(label="HI!", state_color="#23a55a")
        ctx.bus_publish("hello.shouted", {"who": "world"})

if __name__ == "__main__":
    plugin.run()
```

The deck spawns plugins via their manifest, sets `JUBALLER_SOCK` env var, and the SDK
auto-connects to it.
