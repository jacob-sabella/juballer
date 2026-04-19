# discord plugin for juballer-deck

Polls Discord's **public server-widget JSON endpoint** for one server
and renders the voice channels + members on the deck pane named
`discord`.

## Why this approach

Discord's local IPC socket can talk to the desktop client but
voice-state RPC scopes (`rpc.voice.read`) are gated behind Discord's
app-approval process — months of review, not realistic for personal
plugins. The public widget endpoint sidesteps OAuth entirely:
unauthenticated GET, returns voice channel + member presence as JSON.

What you trade off:

- No "speaking" dot — Discord doesn't expose it on the public endpoint.
- Server-scoped, not "what voice channel am I in across all servers".
- Server admin must enable the widget toggle.

## Setup

1. **In Discord** → your server → Server Settings → Widget → enable
   "Server Widget".
2. **Get your Server ID**: Discord Settings → Advanced → toggle
   Developer Mode on. Right-click your server icon → **Copy Server
   ID**.
3. **Set the env var** before launching the deck:
   ```sh
   export DISCORD_GUILD_ID=123456789012345678
   ./target/release/juballer-deck
   ```
   Optional: `DISCORD_POLL_INTERVAL_S` (default `5`).
4. **Add a `dynamic` widget pane** that watches `tree_key = "discord"`:
   ```toml
   [top_panes.discord_pane]
   widget = "dynamic"
   tree_key = "discord"
   placeholder = "discord plugin starting…"
   ```

## Endpoint

```
GET https://discord.com/api/guilds/{DISCORD_GUILD_ID}/widget.json
```

No auth header. Rate limit is generous (default poll is 5s; well under
Discord's threshold).

## Files

| file              | purpose                                            |
|-------------------|----------------------------------------------------|
| `manifest.toml`   | plugin metadata for the deck's plugin host         |
| `main.py`         | the polling + render loop                          |
| `requirements.txt`| `httpx`                                            |
