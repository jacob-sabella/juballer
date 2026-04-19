"""Discord voice-state mirror for juballer-deck via RPC + full OAuth.

Connects to the local Discord desktop client over its IPC socket using
pypresence, authorizes via the standard OAuth dance (AUTHORIZE → token
exchange → AUTHENTICATE), then subscribes to voice events for the
user's currently-selected voice channel.

Setup:
  1. Discord dev portal → your app → App Testers → add yourself.
  2. Same app → OAuth2 → Redirects → add `http://localhost` → Save.
  3. Same app → OAuth2 → Reset Client Secret → copy.
  4. Put both in `<plugin_dir>/.env` as:
       DISCORD_CLIENT_ID=<id>
       DISCORD_CLIENT_SECRET=<secret>
     (file is read directly, no shell sourcing required).
  5. The first run will pop an "Authorize?" dialog in your Discord
     client — click yes once. The access_token is cached at
     `<plugin_dir>/oauth.json` and refreshed automatically.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
from pathlib import Path
from typing import Any, Optional

import httpx
import pypresence
import struct

# pypresence's data_received() parses `data[8:]` as a single JSON object,
# which breaks when Discord batches multiple IPC frames into one TCP read
# (commonly triggered by back-to-back command/response pairs). Replace it
# with a frame-aware version that walks the buffer using the 8-byte
# opcode+length header Discord prepends to each frame.
def _patched_data_received(self, data: bytes) -> None:
    if self.sock_reader._eof:
        return
    self.sock_reader._buffer.extend(data)
    self.sock_reader._wakeup_waiter()
    if (
        self.sock_reader._transport is not None
        and not self.sock_reader._paused
        and len(self.sock_reader._buffer) > 2 * self.sock_reader._limit
    ):
        try:
            self.sock_reader._transport.pause_reading()
        except NotImplementedError:
            self.sock_reader._transport = None
        else:
            self.sock_reader._paused = True
    pos = 0
    while pos + 8 <= len(data):
        _op, length = struct.unpack("<II", data[pos:pos + 8])
        end = pos + 8 + length
        if end > len(data):
            break
        try:
            payload = json.loads(data[pos + 8:end].decode("utf-8"))
        except Exception:
            pos = end
            continue
        pos = end
        if payload.get("evt") is not None:
            evt = payload["evt"].lower()
            if evt in self._events:
                asyncio.create_task(self._events[evt](payload["data"]))
            elif evt == "error":
                # Don't raise — that kills the protocol and stops all events.
                logger.warning("discord error: %s", payload.get("data"))


pypresence.AioClient.on_event = _patched_data_received

from juballer_deck import Plugin
from juballer_deck.view import (
    bg,
    button,
    divider,
    hstack,
    icon_emoji,
    kpi,
    padding,
    text,
    vstack,
)


PLUGIN_DIR = Path(__file__).resolve().parent
ENV_FILE = PLUGIN_DIR / ".env"
TOKEN_FILE = PLUGIN_DIR / "oauth.json"
PANE = "discord"
BADGE_PANE = "discord_badge"
TILE_NAME = "discord_unread"
# rpc.notifications.read is optional — the plugin requests it but gracefully
# degrades to voice-only mode if Discord refuses. The user can revoke + re-auth
# (delete plugins/discord/oauth.json) to try again.
SCOPES = ["rpc", "rpc.voice.read", "rpc.notifications.read", "identify"]
REDIRECT_URI = "http://localhost"
TOKEN_URL = "https://discord.com/api/v10/oauth2/token"
NOTIFICATION_RING_CAP = 20
RECENT_NOTIFS_IN_TREE = 5

logger = logging.getLogger("juballer.discord")


def _load_env() -> dict[str, str]:
    out: dict[str, str] = {}
    if ENV_FILE.exists():
        for ln in ENV_FILE.read_text().splitlines():
            ln = ln.strip()
            if not ln or ln.startswith("#"):
                continue
            if "=" not in ln:
                continue
            k, v = ln.split("=", 1)
            out[k.strip()] = v.strip()
    for k in ("DISCORD_CLIENT_ID", "DISCORD_CLIENT_SECRET"):
        if k in os.environ:
            out[k] = os.environ[k]
    return out


_env = _load_env()
CLIENT_ID = _env.get("DISCORD_CLIENT_ID")
CLIENT_SECRET = _env.get("DISCORD_CLIENT_SECRET")

plugin = Plugin("discord")

state: dict[str, Any] = {
    "guild_name": None,
    "channel_name": None,
    "channel_id": None,
    "self_id": None,
    "members": {},
    "status": None,
}

# guild_id -> {"name": str, "unread": int}
guilds: dict[str, dict] = {}

# Ring of recent notifications, newest last. Each entry:
#   {"title": str, "body": str, "channel_id": str|None,
#    "channel_name": str|None, "icon_url": str|None, "ts": float}
notifications: list[dict] = []

_last_pushed: Optional[dict] = None
_last_pushed_badge: Optional[dict] = None
_last_pushed_unread: Optional[int] = None
rpc: Optional[pypresence.AioClient] = None
_subscribed_channel_id: Optional[str] = None
_notifications_subscribed: bool = False


def _total_unread() -> int:
    return sum(int(g.get("unread") or 0) for g in guilds.values())


def _truncate(s: str, n: int = 64) -> str:
    if s is None:
        return ""
    s = str(s).strip().replace("\n", " ")
    return s if len(s) <= n else s[: n - 1] + "…"


def _voice_block() -> Optional[dict]:
    """Return a compact voice-channel block, or None if not in a voice channel."""
    if state["status"] or not state["channel_name"]:
        return None
    member_rows: list[dict] = []
    for m in state["members"].values():
        speaking = m.get("speaking")
        nick = m.get("nick") or m.get("username") or "?"
        row = [
            icon_emoji("●" if speaking else "○", size=10),
            text(
                nick,
                size=12,
                color="text" if speaking else "subtext1",
                weight="bold" if speaking else None,
            ),
        ]
        if m.get("self_mute"):
            row.append(icon_emoji("🔇", size=10))
        if m.get("self_deaf"):
            row.append(icon_emoji("🎧", size=10))
        member_rows.append(
            padding(hstack(*row, gap=6, align="center"), top=2, bottom=2, left=4, right=4)
        )

    header = vstack(
        hstack(
            icon_emoji("🔊", size=16),
            text(state["channel_name"], size=15, weight="bold", color="blue"),
            gap=6,
            align="center",
        ),
        text(state["guild_name"] or "", size=10, color="subtext0"),
        gap=2,
    )
    return vstack(header, *member_rows, gap=4)


def _unread_kpi_block(total: int) -> dict:
    return kpi(
        str(total),
        label="unread mentions/DMs",
        color="red" if total else "subtext0",
    )


def _guilds_block() -> Optional[dict]:
    # Only show guilds with unread > 0, capped to 6 rows. User is in too many
    # servers to list them all — and a zero-unread guild has nothing to say.
    items = [g for g in guilds.values() if int(g.get("unread") or 0) > 0]
    items.sort(key=lambda g: (-(int(g.get("unread") or 0)), (g.get("name") or "").lower()))
    if not items:
        return None
    MAX = 6
    rows: list[dict] = []
    for g in items[:MAX]:
        unread = int(g.get("unread") or 0)
        rows.append(
            padding(
                hstack(
                    text(_truncate(g.get("name") or "?", 24), size=12, color="text"),
                    text(str(unread), size=11, weight="bold", color="red"),
                    gap=8,
                    align="center",
                ),
                top=2, bottom=2, left=4, right=4,
            )
        )
    if len(items) > MAX:
        rows.append(text(f"+ {len(items) - MAX} more…", size=10, color="subtext0"))
    return vstack(
        text(f"Unread servers · {len(items)}", size=11, weight="bold", color="subtext0"),
        *rows,
        gap=2,
    )


def _notifications_block() -> Optional[dict]:
    recent = notifications[-RECENT_NOTIFS_IN_TREE:]
    if not recent:
        return None
    rows: list[dict] = []
    for n in reversed(recent):  # newest first
        title = _truncate(n.get("title") or "", 40)
        body = _truncate(n.get("body") or "", 64)
        channel_name = n.get("channel_name")
        header_children: list[dict] = [
            icon_emoji("🔔", size=11),
            text(title, size=12, weight="bold", color="text"),
        ]
        if channel_name:
            header_children.append(text(f"#{channel_name}", size=10, color="subtext0"))
        rows.append(
            vstack(
                hstack(*header_children, gap=6, align="center"),
                text(body, size=11, color="subtext1"),
                gap=2,
            )
        )
    return vstack(
        text("Recent", size=11, weight="bold", color="subtext0"),
        *rows,
        gap=6,
    )


def render_tree() -> dict:
    if state["status"]:
        return padding(
            bg(
                vstack(
                    text("discord", size=14, weight="bold", color="text"),
                    text(state["status"], size=11, color="subtext0"),
                    gap=4,
                ),
                color="surface0",
                rounding=6,
            ),
            all=8,
        )

    blocks: list[dict] = []
    voice = _voice_block()
    if voice is not None:
        blocks.append(voice)
        blocks.append(divider())
    else:
        blocks.append(
            vstack(
                text("discord", size=14, weight="bold", color="text"),
                text("not in voice", size=11, color="subtext0"),
                gap=4,
            )
        )
        blocks.append(divider())

    blocks.append(_unread_kpi_block(_total_unread()))

    guild_block = _guilds_block()
    if guild_block is not None:
        blocks.append(divider())
        blocks.append(guild_block)

    notif_block = _notifications_block()
    if notif_block is not None:
        blocks.append(divider())
        blocks.append(notif_block)

    return padding(
        bg(vstack(*blocks, gap=6), color="surface0", rounding=6),
        all=8,
    )


def render_badge() -> dict:
    total = _total_unread()
    if total:
        body = hstack(
            icon_emoji("💬", size=16),
            text(f"{total} unread", size=13, weight="bold", color="red"),
            button("open", "deck.page_goto", {"page": "discord:overview"}),
            gap=8,
            align="center",
        )
        return padding(
            bg(body, color="surface0", rounding=6),
            all=8,
        )
    # Dim state when zero unread.
    body = hstack(
        icon_emoji("💬", size=14),
        text("no unread", size=12, color="overlay0"),
        button("open", "deck.page_goto", {"page": "discord:overview"}),
        gap=8,
        align="center",
    )
    return padding(
        bg(body, color="mantle", rounding=6),
        all=8,
    )


async def push_all() -> None:
    """Push both the rich `discord` pane and the compact `discord_badge` pane
    and update the named tile. Each side is pushed only if its payload
    actually changed to avoid NDJSON noise."""
    global _last_pushed, _last_pushed_badge, _last_pushed_unread

    tree = render_tree()
    if tree != _last_pushed:
        _last_pushed = tree
        try:
            await plugin.push_view(PANE, tree)
        except Exception:
            logger.exception("push_view(%s) failed", PANE)

    badge = render_badge()
    if badge != _last_pushed_badge:
        _last_pushed_badge = badge
        try:
            await plugin.push_view(BADGE_PANE, badge)
        except Exception:
            logger.exception("push_view(%s) failed", BADGE_PANE)

    total = _total_unread()
    if total != _last_pushed_unread:
        _last_pushed_unread = total
        try:
            if total > 0:
                await plugin.set_named_tile(
                    TILE_NAME,
                    icon="💬",
                    label=f"{total} DM" if total == 1 else f"{total} DMs",
                    state_color="red",
                )
            else:
                # Back to the config default.
                await plugin.set_named_tile(TILE_NAME, clear=True)
        except Exception:
            logger.exception("set_named_tile failed")


# Legacy shim for voice handlers that used to call push_if_changed().
async def push_if_changed() -> None:
    await push_all()


async def _exchange_code(http: httpx.AsyncClient, code: str) -> dict:
    r = await http.post(TOKEN_URL, data={
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": REDIRECT_URI,
    })
    r.raise_for_status()
    tok = r.json()
    tok["obtained_at"] = int(time.time())
    return tok


async def _refresh_token(http: httpx.AsyncClient, refresh: str) -> dict:
    r = await http.post(TOKEN_URL, data={
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
        "grant_type": "refresh_token",
        "refresh_token": refresh,
    })
    r.raise_for_status()
    tok = r.json()
    tok["obtained_at"] = int(time.time())
    return tok


def _save_token(tok: dict) -> None:
    TOKEN_FILE.write_text(json.dumps(tok, indent=2))
    TOKEN_FILE.chmod(0o600)


def _load_token() -> Optional[dict]:
    if not TOKEN_FILE.exists():
        return None
    try:
        return json.loads(TOKEN_FILE.read_text())
    except Exception:
        return None


def _is_token_fresh(tok: dict) -> bool:
    return (tok.get("obtained_at", 0) + tok.get("expires_in", 0) - 60) > int(time.time())


async def _ensure_authenticated(http: httpx.AsyncClient) -> str:
    """Return a usable access_token, refreshing or doing the full dance as needed."""
    tok = _load_token()
    if tok and _is_token_fresh(tok):
        return tok["access_token"]
    if tok and "refresh_token" in tok:
        try:
            tok = await _refresh_token(http, tok["refresh_token"])
            _save_token(tok)
            return tok["access_token"]
        except Exception as e:  # noqa: BLE001
            logger.warning("refresh failed: %s — falling back to AUTHORIZE", e)
    code_resp = await rpc.authorize(CLIENT_ID, SCOPES)
    code = code_resp["data"]["code"]
    tok = await _exchange_code(http, code)
    _save_token(tok)
    return tok["access_token"]


# Cache: channel_id -> {"name": str, "guild_id": str, "guild_name": str}
_channel_cache: dict[str, dict] = {}


async def _seed_channel_cache() -> None:
    """One-shot: fetch all guilds the user is in + their channels, cache id->name."""
    try:
        resp = await rpc.get_guilds()
    except Exception:
        return
    for g in (resp.get("data") or {}).get("guilds") or []:
        gid = g.get("id")
        gname = g.get("name")
        if not gid:
            continue
        # Seed the guilds map with initial zero-unread state so the overview
        # has something to show before the first GUILD_STATUS event arrives.
        guilds.setdefault(gid, {"name": gname, "unread": 0})
        if gname:
            guilds[gid]["name"] = gname
        try:
            chans = await rpc.get_channels(gid)
        except Exception:
            continue
        for c in (chans.get("data") or {}).get("channels") or []:
            cid = c.get("id")
            if cid:
                _channel_cache[cid] = {
                    "name": c.get("name") or "?",
                    "guild_id": gid,
                    "guild_name": gname,
                }


async def _subscribe_notifications_and_guilds() -> None:
    """Register NOTIFICATION_CREATE + per-guild GUILD_STATUS listeners.

    If rpc.notifications.read was refused during OAuth, the notification
    subscribe call will raise — we swallow the error and continue in
    voice-only + guild-unread mode. Per-guild GUILD_STATUS is cheap to
    register for every known guild."""
    global _notifications_subscribed
    try:
        await rpc.register_event("NOTIFICATION_CREATE", _on_notification)
        _notifications_subscribed = True
    except Exception as e:  # noqa: BLE001
        logger.warning(
            "NOTIFICATION_CREATE subscribe failed (is rpc.notifications.read granted?): %s", e
        )
        _notifications_subscribed = False

    for gid in list(guilds.keys()):
        try:
            await rpc.register_event("GUILD_STATUS", _on_guild_status, args={"guild_id": gid})
        except Exception as e:  # noqa: BLE001
            logger.warning("GUILD_STATUS subscribe failed for guild %s: %s", gid, e)


# ---- RPC event handlers ----

async def _on_voice_channel_select(data: dict) -> None:
    ch_id = data.get("channel_id")
    await _resubscribe(ch_id)


async def _enter_channel(channel_id: str) -> None:
    state["channel_id"] = channel_id
    cached = _channel_cache.get(channel_id)
    if cached:
        state["channel_name"] = cached["name"]
        state["guild_name"] = cached.get("guild_name")
    else:
        state["channel_name"] = "?"
        state["guild_name"] = None
    state["members"] = {}
    await _subscribe_voice(channel_id)
    await push_if_changed()


async def _resubscribe(new_channel_id: Optional[str]) -> None:
    global _subscribed_channel_id
    if _subscribed_channel_id == new_channel_id:
        return
    if _subscribed_channel_id:
        for evt in ("VOICE_STATE_CREATE", "VOICE_STATE_UPDATE", "VOICE_STATE_DELETE",
                    "SPEAKING_START", "SPEAKING_STOP"):
            try:
                await rpc.unsubscribe(evt, channel_id=_subscribed_channel_id)
            except Exception:
                pass
    _subscribed_channel_id = new_channel_id
    if new_channel_id:
        await _enter_channel(new_channel_id)
    else:
        state["channel_id"] = None
        state["channel_name"] = None
        state["members"] = {}
        await push_if_changed()


async def _subscribe_voice(channel_id: str) -> None:
    pairs = [
        ("VOICE_STATE_CREATE", _on_voice_state_create),
        ("VOICE_STATE_UPDATE", _on_voice_state_update),
        ("VOICE_STATE_DELETE", _on_voice_state_delete),
        ("SPEAKING_START", _on_speaking_start),
        ("SPEAKING_STOP", _on_speaking_stop),
    ]
    for evt, cb in pairs:
        try:
            await rpc.register_event(evt, cb, args={"channel_id": channel_id})
        except Exception as e:  # noqa: BLE001
            logger.warning("subscribe %s failed: %s", evt, e)


async def _on_voice_state_create(d: dict) -> None:
    u = d.get("user") or {}
    if u.get("id"):
        state["members"][u["id"]] = {
            "username": u.get("username"),
            "nick": d.get("nick"),
            "speaking": False,
            "self_mute": (d.get("voice_state") or {}).get("self_mute", False),
            "self_deaf": (d.get("voice_state") or {}).get("self_deaf", False),
        }
        await push_if_changed()


async def _on_voice_state_update(d: dict) -> None:
    u = d.get("user") or {}
    uid = u.get("id")
    if not uid:
        return
    m = state["members"].setdefault(uid, {"username": u.get("username")})
    m["nick"] = d.get("nick")
    m["self_mute"] = (d.get("voice_state") or {}).get("self_mute", False)
    m["self_deaf"] = (d.get("voice_state") or {}).get("self_deaf", False)
    await push_if_changed()


async def _on_voice_state_delete(d: dict) -> None:
    u = d.get("user") or {}
    if u.get("id") and u["id"] in state["members"]:
        del state["members"][u["id"]]
        await push_if_changed()


async def _on_speaking_start(d: dict) -> None:
    uid = d.get("user_id")
    if uid and uid in state["members"]:
        state["members"][uid]["speaking"] = True
        await push_if_changed()


async def _on_speaking_stop(d: dict) -> None:
    uid = d.get("user_id")
    if uid and uid in state["members"]:
        state["members"][uid]["speaking"] = False
        await push_if_changed()


async def _on_guild_status(d: dict) -> None:
    """GUILD_STATUS — Discord pushes the current guild + an unread_count
    whenever activity changes in that guild. We just mirror unread_count."""
    g = d.get("guild") or {}
    gid = g.get("id")
    if not gid:
        return
    entry = guilds.setdefault(gid, {"name": g.get("name"), "unread": 0})
    if g.get("name"):
        entry["name"] = g["name"]
    # Discord sends either unread_count (ints) or mentions/message counts
    # depending on account — treat any non-None numeric field as unread.
    unread = d.get("unread_count")
    if unread is None:
        unread = d.get("message_count") or d.get("mentions") or 0
    try:
        entry["unread"] = int(unread)
    except (TypeError, ValueError):
        entry["unread"] = 0
    await push_all()


async def _on_notification(d: dict) -> None:
    """NOTIFICATION_CREATE — a new toast from any channel. We append to a
    capped ring so the overview can show recent context."""
    title = d.get("title")
    body = d.get("body") or d.get("content") or ""
    channel_id = d.get("channel_id")
    channel_name = None
    if channel_id and channel_id in _channel_cache:
        channel_name = _channel_cache[channel_id].get("name")
    icon_url = d.get("icon_url") or (d.get("message") or {}).get("author", {}).get("avatar")
    notifications.append({
        "title": title,
        "body": body,
        "channel_id": channel_id,
        "channel_name": channel_name,
        "icon_url": icon_url,
        "ts": time.time(),
    })
    if len(notifications) > NOTIFICATION_RING_CAP:
        del notifications[: len(notifications) - NOTIFICATION_RING_CAP]
    await push_all()


async def main() -> None:
    global rpc

    sock_path = os.environ.get("JUBALLER_SOCK")
    if sock_path:
        asyncio.create_task(plugin._run(sock_path))  # noqa: SLF001

    if not CLIENT_ID or not CLIENT_SECRET:
        state["status"] = "DISCORD_CLIENT_ID + DISCORD_CLIENT_SECRET missing in .env"
        await push_if_changed()
        while True:
            await asyncio.sleep(60)

    rpc = pypresence.AioClient(CLIENT_ID)
    try:
        await rpc.start()
    except Exception as e:  # noqa: BLE001
        state["status"] = f"Discord IPC unavailable: {e}"
        await push_if_changed()
        while True:
            await asyncio.sleep(60)

    async with httpx.AsyncClient() as http:
        try:
            token = await _ensure_authenticated(http)
            await rpc.authenticate(token)
        except Exception as e:  # noqa: BLE001
            state["status"] = f"OAuth failed: {e} (check tester list + redirect_uri)"
            await push_if_changed()
            while True:
                await asyncio.sleep(60)

        # Build channel name cache before subscribing — done once when no event
        # traffic is contending with command responses.
        await _seed_channel_cache()
        await _subscribe_notifications_and_guilds()

        try:
            await rpc.register_event("VOICE_CHANNEL_SELECT", _on_voice_channel_select)
        except Exception as e:  # noqa: BLE001
            logger.warning("register VOICE_CHANNEL_SELECT failed: %s", e)

        try:
            sel = await rpc.get_selected_voice_channel()
            d = (sel or {}).get("data") or {}
            ch_id = d.get("id")
            if ch_id:
                # Cache from this initial response too, in case seed missed it.
                if ch_id not in _channel_cache:
                    _channel_cache[ch_id] = {
                        "name": d.get("name") or "?",
                        "guild_id": d.get("guild_id"),
                        "guild_name": None,
                    }
                await _resubscribe(ch_id)
            else:
                await push_if_changed()
        except Exception:
            await push_if_changed()

        while True:
            await asyncio.sleep(3600)


if __name__ == "__main__":
    asyncio.run(main())
