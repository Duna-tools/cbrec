# Chaturbate API Map

> Captured via Chrome DevTools MCP — 2026-06-07  
> Base: `https://chaturbate.com` (English)  
> Auth required: `sessionid` + `csrftoken` cookies

---

## Authentication

All requests require session cookies. CSRF token must be sent as:
- Header: `X-Requested-With: XMLHttpRequest`
- POST body field: `csrfmiddlewaretoken=<token>`

### Cookies
| Cookie | Source | Purpose |
|---|---|---|
| `sessionid` | Login | Session auth |
| `csrftoken` | Login | CSRF protection |
| `language=en` | Set manually | Force English |
| `en_subdomain=1` | Set manually | Use `chaturbate.com` (no `es.` prefix) |
| `sbr` | Login | Session binding |
| `cf_clearance` | Cloudflare | Bot protection |

### Force English
```
document.cookie = "language=en; domain=.chaturbate.com; path=/";
document.cookie = "en_subdomain=1; domain=.chaturbate.com; path=/";
```
Then navigate to `https://en.chaturbate.com/` → redirects to `https://chaturbate.com/`.

---

## REST API Endpoints

Base: `https://chaturbate.com`

### Room List & Discovery

| Endpoint | Method | Params | Description |
|---|---|---|---|
| `/api/ts/roomlist/room-list/` | GET | `limit`, `offset`, `hashtags` | Paginated room list |
| `/api/ts/roomlist/all-tags/` | GET | `limit`, `offset`, `hashtags` | All available tags |
| `/api/ts/hashtags/top_tags/` | GET | `count` | Top hashtags |
| `/api/ts/hashtags/approved_from_tags_list/` | GET | `tags` (comma-separated) | Validate tags |
| `/api/more_like/{username}/` | GET | — | Similar rooms to broadcaster |

#### Room List Response Shape
```json
{
  "rooms": [{
    "username": "string",
    "display_age": 22,
    "gender": "f",
    "location": "string",
    "current_show": "public",
    "room_subject": "string",
    "tags": ["string"],
    "is_new": false,
    "num_users": 1234,
    "num_followers": 56789,
    "start_dt_utc": "2026-06-07T04:11:24+00:00",
    "start_timestamp": 1780805484,
    "has_password": false,
    "private_price": 60,
    "spy_show_price": 30,
    "is_gaming": false,
    "is_age_verified": true,
    "is_following": false,
    "label": "new|public|promoted",
    "source_name": "df|rc|pr",
    "img": "https://thumb.live.mmcdn.com/riw/{username}.jpg"
  }]
}
```

---

### Room Context & Info

| Endpoint | Method | Description |
|---|---|---|
| `/api/panel_context/{username}/` | GET | Broadcaster panel widget (tip goal, highest tip, latest tip) |
| `/api/biocontext/{username}/` | GET | Bio and profile info |
| `/api/ts/chatmessages/user_info/{username}/?room={room}` | GET | Viewer's status in room (mod, fanclub, tokens) |
| `/api/ts/games/current/room/{username}` | GET | Active game in room |
| `/api/public/asp/broadcast/applist/{broadcaster_uid}/` | GET | Broadcaster's active apps |
| `/api/public/asp/shortcuts/{broadcaster_uid}/` | GET | Broadcaster shortcut commands (may 404) |
| `/promotion/api/promote_price/?slug={username}` | GET | Cost to promote room |

#### panel_context Response Example
```json
{
  "row1_label": "Tip Received / Goal :",
  "row1_value": "2999 / 2999",
  "row2_label": "Highest Tip:",
  "row2_value": "username (500)",
  "row3_label": "Latest Tip Received:",
  "row3_value": "username (20)",
  "template": "3_rows_of_labels",
  "name": "Tip Goal"
}
```

---

### Chat & Messaging

| Endpoint | Method | Params | Description |
|---|---|---|---|
| `/api/ts/chat/message-render-options/` | GET | — | Chat render config (fonts, colors) |
| `/api/ts/chat/ignored-users/` | GET | — | Blocked users list |
| `/api/ts/chat/send-player-quality/` | POST | `quality`, `room` | Report video quality selection |
| `/api/ts/chatmessages/pm_list/{username}/` | GET | `offset`, `room` | Private messages |
| `/api/ts/chatmessages/media/` | GET | `media_type=I`, `limit`, `offset` | Chat media (images) |
| `/api/getchatuserlist/` | GET | `roomname`, `private`, `sort_by`, `exclude_staff` | Live user list in room |
| `/api/notes/usernames/` | GET | — | Personal notes on users |
| `/api/messaging/unread/` | GET | — | Unread message count |
| `/api/messaging/profile/{username}/` | GET | — | User profile for DMs |
| `/api/messaging/preferences/` | GET | — | Messaging settings |
| `/push_service/publish_chat_message_live/` | POST | `room`, `message`, `csrfmiddlewaretoken` | Send chat message (requires tokens; 403 for zero-balance) |
| `/push_service/room_user_count/{username}/` | POST | `presence_id` | Room user presence count |

---

### Following

| Endpoint | Method | Body | Description |
|---|---|---|---|
| `/follow/api/online_followed_rooms/` | GET | — | Followed rooms currently live |
| `/follow/follow/{username}/` | POST | `csrfmiddlewaretoken` | Follow a broadcaster |
| `/follow/unfollow/{username}/` | POST | `csrfmiddlewaretoken` | Unfollow a broadcaster |

#### online_followed_rooms Response
```json
{
  "online": 7,
  "total": 111,
  "online_rooms": [
    {
      "room": "username",
      "image": "https://thumb.live.mmcdn.com/riw/{username}.jpg"
    }
  ]
}
```

#### follow / unfollow Response
```json
{
  "following": true,
  "notification_frequency": "smart"
}
```

> `notification_frequency` values: `"smart"` (default), likely also `"always"` / `"never"`.  
> Broadcaster gets notified of new followers via `BellNotificationTopic` on their own user channel.

---

### Tipping

| Endpoint | Method | Description |
|---|---|---|
| `/tipping/send_tip/{username}/` | POST | Send tip (custom amount or tip-menu item) |
| `/tipping/current_tokens/?room={room}` | GET | Token balance + tip dialog init data |
| `/tipping/rate_model/{username}/` | GET | Tip rate model for room |
| `/tipping/add_comment/{username}/` | POST | Add tip comment to room |
| `/tipping/tips_in_last_24/` | GET | Whether viewer tipped in last 24h |
| `/tipping/crypto_invoices/` | GET/POST | Crypto tip invoices |
| `/api/ts/tipping/` | GET | Tipping overview |
| `/api/ts/tipping/memberships/` | GET | Token membership plans |
| `/tipping/memberships_panel/` | GET | Memberships panel widget |
| `/api/ts/tipping/token-stats/` | GET | Token stats |
| `/api/ts/tipping/rating-history/` | GET | Tip rating history |
| `/tipping/csv/history/` | GET | Download tip history CSV |
| `/tipping/cashout_tokens/` | GET/POST | Token cashout flow |
| `/contest/log/{username}/` | GET | Contest/goal access check for room |

#### POST /tipping/send_tip/{username}/ — Request Body
```json
{
  "tip_amount": 10,
  "message": "",
  "tip_room_type": "public",
  "tip_type": "public",
  "video_mode": "normal",
  "from_username": "your_username",
  "anonymous": false,
  "sig": "<signature>",
  "source": "tip_menu_custom"
}
```
For tip-menu items, add: `menu_id`, `menu_name`, `item_id`, `item_name`, `price_type` — and use `source: "tip_menu"`.

> `tip_room_type` values: `"public"`, `"private"`, `"sitewidePMs"`  
> `tip_type` values: `"public"`, `"anonymous"`  
> `source` values: `"tip_menu_custom"` (custom amount), `"tip_menu"` (menu item)

#### POST /tipping/send_tip/{username}/ — Response
```json
{
  "success": true,
  "token_balance": 490,
  "error": null,
  "tipped_performer_last_24hrs": true,
  "show_purchase_tokens": false
}
```

---

### Fan Club

| Endpoint | Method | Description |
|---|---|---|
| `/fanclub/join/{username}/` | GET | Open fan club join page (redirects to purchase flow) |
| `/fanclub/process/{username}/` | POST | Process fan club subscription |

#### POST /fanclub/process/{username}/ — Body
```
subscription_signup=1
csrfmiddlewaretoken=<token>
cost=<price_in_usd_cents>
token_signup=1
tokens=<monthly_token_cost>
months=1
```

#### Query params for /fanclub/join/
```
?source=SupporterSourceJoinFanClubButton
```

---

### Search & Discovery

| Endpoint | Method | Params | Description |
|---|---|---|---|
| `/api/ts/roomlist/room-list/` | GET | `search={term}` | Search rooms by text |
| `/api/ts/roomlist/room-list/` | GET | `private=true` | Rooms currently in private/spy show |
| `/api/ts/roomlist/room-list/` | GET | `hidden=true&private=false` | Hidden rooms |

---

### Notifications

| Endpoint | Method | Params | Description |
|---|---|---|---|
| `/notifications/updates/` | GET | `notification_type` (multi) | Bell notifications, offline tips, twitter feed |

```
notification_type=twitter_feed
notification_type=offline_tip
notification_type=bell_notification
```

---

## Real-Time Push Service (Ably)

Chaturbate uses **Ably** hosted on their own infrastructure (`highwebmedia.com`).

### Connection Flow

**Step 1 — Get Ably token:**
```
POST /push_service/auth/
Content-Type: multipart/form-data

presence_id=<random_string>
topics=<JSON map — see Topics below>
backend=a
csrfmiddlewaretoken=<token>
```

**Step 2 — Get room history:**
```
POST /push_service/room_history/
Content-Type: multipart/form-data

topics=<subset of room topics>
csrfmiddlewaretoken=<token>
```

**Step 3 — Connect WebSocket:**
```
wss://realtime.pa.highwebmedia.com
```
Fallbacks:
```
a-fallback.pa.highwebmedia.com
b-fallback.pa.highwebmedia.com
c-fallback.pa.highwebmedia.com
d-fallback.pa.highwebmedia.com
e-fallback.pa.highwebmedia.com
```
REST: `https://realtime.pa.highwebmedia.com`

### Auth Response Shape
```json
{
  "token": "<JWT>",
  "channels": { "<TopicName>": "<ably_channel>" },
  "failures": {},
  "client_id": "<presence_id>-<user_uid>",
  "settings": {
    "backend": "a",
    "host": "realtime.pa.highwebmedia.com",
    "flags": { "is_live": true }
  }
}
```

### Topic → Ably Channel Map

> `{b_uid}` = broadcaster UID (e.g. `MS6JBC5`)  
> `{u_uid}` = viewer UID (e.g. `FTKAA7L`)

| Topic | Ably Channel | Payload |
|---|---|---|
| `RoomMessageTopic` | `room:grouped:{b_uid}:0` | Chat message: text, font, color, from_user |
| `RoomTipAlertTopic` | `room:grouped:{b_uid}:0` | Tip: amount, from_username, message, anon flag |
| `RoomTipGoalProgressTopic` | `room:grouped:{b_uid}:0` | Goal progress |
| `RoomTipMenuTopic` | `room:grouped:{b_uid}:0` | Tip menu update |
| `RoomStatusTopic` | `room:grouped:{b_uid}:0` | Room status change (public/private/offline) |
| `RoomTitleChangeTopic` | `room:grouped:{b_uid}:0` | Subject/title change |
| `RoomEnterLeaveTopic` | `room:grouped:{b_uid}:0` | Viewer enters/leaves |
| `RoomKickTopic` | `room:grouped:{b_uid}:0` | User kicked/banned |
| `RoomSilenceTopic` | `room:grouped:{b_uid}:0` | User silenced |
| `RoomNoticeTopic` | `room:grouped:{b_uid}:0` | Room notice/announcement |
| `RoomPasswordProtectedTopic` | `room:grouped:{b_uid}:0` | Password protection change |
| `RoomModeratorPromotedTopic` | `room:grouped:{b_uid}:0` | New moderator |
| `RoomModeratorRevokedTopic` | `room:grouped:{b_uid}:0` | Mod removed |
| `RoomUpdateTopic` | `room:grouped:{b_uid}:0` | Generic room update |
| `RoomSettingsTopic` | `room:grouped:{b_uid}:0` | Room settings change |
| `RoomPurchaseTopic` | `room:grouped:{b_uid}:0` | Purchase event |
| `QualityUpdateTopic` | `room:grouped:{b_uid}:0` | Stream quality change |
| `LatencyUpdateTopic` | `room:grouped:{b_uid}:0` | Stream latency change |
| `GameUpdateTopic` | `room:grouped:{b_uid}:0` | Active game update |
| `ViewerPromotionTopic` | `room:grouped:{b_uid}:0` | Viewer promoted |
| `RoomFanClubJoinedTopic` | `room:fanclub:{b_uid}` | Fan club join |
| `RoomShortcodeTopic` | `room:shortcode:{b_uid}` | Shortcode/command event |
| `RoomUserPresenceTopic` | `room_user:grouped:{b_uid}:{u_uid}:0` | Presence (requires `presence` capability) |
| `RoomUserNoticeTopic` | `room_user:grouped:{b_uid}:{u_uid}:0` | Personal notice to viewer |
| `RoomUserPrivateStatusTopic` | `room_user:grouped:{b_uid}:{u_uid}:0` | Viewer's private show status |
| `RoomUserHiddenCamStatusTopic` | `room_user:grouped:{b_uid}:{u_uid}:0` | Hidden cam status |
| `UserTokenUpdateTopic` | `user:grouped:{u_uid}` | Token balance update |
| `UserAlertTopic` | `user:grouped:{u_uid}` | User alert |
| `UserLowBalanceTopic` | `user:grouped:{u_uid}` | Low balance warning |
| `UserOneClickTopic` | `user:grouped:{u_uid}` | One-click tip event |
| `UserAutoRefillAttemptTopic` | `user:grouped:{u_uid}` | Auto-refill attempt |
| `UserColorUpdateTopic` | `user:grouped:{u_uid}` | Chat color update |
| `UserChatMediaOpenedTopic` | `user:grouped:{u_uid}` | Media opened in chat |
| `UserChatMediaRemovedTopic` | `user:grouped:{u_uid}` | Media removed from chat |
| `UserSMCWatchingTopic` | `user:grouped:{u_uid}` | Social media watching |
| `UserNewsSeenTopic` | `user:grouped:{u_uid}` | News/updates seen |
| `OfflineTipNotificationTopic` | `user:grouped:{u_uid}` | Offline tip notification |
| `UpdateOfflineTipNotificationTopic` | `user:grouped:{u_uid}` | Update to offline tip |
| `BellNotificationTopic` | `user:grouped:{u_uid}` | Bell notification |
| `GlobalPushServiceBackendChangeTopic` | `global:push_service` | Backend change (reconnect signal) |

### Message Event Shape (chat)
```json
{
  "tid": "uuid-v7",
  "ts": 1780828297.0355,
  "_topic": "RoomMessageTopic",
  "message": "string",
  "font_family": "default",
  "font_color": "rgb(73,73,73)",
  "id": "JWTKTEVX5X06K5",
  "background": "",
  "from_user": {
    "username": "string",
    "gender": "m|f|c|s",
    "is_broadcaster": false,
    "in_fanclub": false,
    "is_following": false,
    "is_mod": false,
    "has_tokens": true,
    "tipped_recently": false,
    "tipped_alot_recently": false,
    "tipped_tons_recently": false
  },
  "method": "lazy"
}
```

### Tip Alert Event Shape
```json
{
  "tid": "uuid-v7",
  "ts": 1780828329.611,
  "_topic": "RoomTipAlertTopic",
  "amount": 10,
  "message": "",
  "history": true,
  "is_anonymous_tip": false,
  "to_username": "broadcaster",
  "from_username": "tipper",
  "gender": "m",
  "is_broadcaster": false,
  "in_fanclub": true,
  "is_following": true,
  "is_mod": false,
  "has_tokens": true,
  "tipped_recently": true,
  "tipped_alot_recently": false,
  "tipped_tons_recently": false,
  "method": "lazy"
}
```

---

## Video Streaming (LLHLS)

### Stream Token
Obtained from page JS (embedded in `<script>` on room page or via push service context).

### Manifest URL
```
https://edge{N}-{region}.live.mmcdn.com/v1/edge/streams/origin.{username}.{streamId}/llhls.m3u8?token={JWE}
```

Example:
```
https://edge25-phx.live.mmcdn.com/v1/edge/streams/origin.auroralowen.01KTG4EAVD8QG6VHGZCE6BXHEZ/llhls.m3u8?token=<JWE>
```

### Chunklist (video/audio)
```
/chunklist_{trackId}_{type}_{streamHash}_llhls.m3u8?session={uuid}&_HLS_msn={seq}&_HLS_part={part}
```

### Segment Types
| Pattern | Description |
|---|---|
| `part_{track}_{seq}_{part}_{type}_{hash}_llhls.m4s` | LL-HLS partial segment |
| `seg_{track}_{seq}_{type}_{hash}_llhls.m4s` | Full segment |
| `init_{track}_{type}_{hash}_llhls.m4s` | Init segment (MP4 moov box) |

### Edge Regions Seen
- `edge2-phx` — Phoenix AZ
- `edge25-phx` — Phoenix AZ

---

## Static Assets

| URL | Description |
|---|---|
| `https://thumb.live.mmcdn.com/riw/{username}.jpg` | Live room thumbnail |
| `https://static-pub.highwebmedia.com/uploads/avatar/...` | User avatars |
| `https://web.static.mmcdn.com/tsdefaultassets/sounds/classic/{huge,large,medium,small,tiny}.mp3` | Tip sounds |

---

## Analytics / Telemetry (not useful for features)

| Domain | Purpose |
|---|---|
| `nwr.mmcdn.com` | New Relic custom telemetry |
| `www.google-analytics.com` | GA4 |
| `token.chaturbate.com` | Internal token service |

---

## Broadcaster APIs

> Require a broadcaster session — 404/403 for viewer accounts.

| Endpoint | Method | Description |
|---|---|---|
| `/api/public/asp/broadcast/applist/{broadcaster_uid}/` | GET | Active apps on broadcaster's room |
| `/api/public/asp/shortcuts/{broadcaster_uid}/` | GET | Broadcaster shortcut commands |
| `/api/ts/tipping/memberships/` | GET | Fan club membership tiers |
| `/tipping/cashout_tokens/` | POST | Cashout tokens to USD |

---

## Private / Spy Shows

| Endpoint | Method | Params | Description |
|---|---|---|---|
| `/api/ts/roomlist/room-list/` | GET | `private=true` | Rooms actively in private show (spy-on-cams eligible) |

> Private show initiation endpoint not observed (requires room in active private show state).  
> `spy_show_price` in room-list payload = tokens/min to spy.  
> `private_price` = tokens/min for private show with broadcaster.  
> Push event `RoomUserPrivateStatusTopic` (channel `room_user:grouped:{b_uid}:{u_uid}:0`) fires when private show status changes.

---

## User Profile

| Endpoint | Method | Description |
|---|---|---|
| `/api/biocontext/{username}/` | GET | Full bio + profile data |
| `/api/ts/chatmessages/user_info/{username}/?room={room}` | GET | Viewer's status in specific room |
| `/api/messaging/profile/{username}/` | GET | DM-context user profile |
| `/b/{username}` | GET (HTML) | Public broadcaster profile page |

---

## Notes

- **broadcaster_uid** and **user_uid** are short alphanumeric IDs (e.g. `MS6JBC5`, `FTKAA7L`), not usernames. Obtained from push_service/auth response or page JS.
- **presence_id** in push_service/auth is a random string; format seen: `+l9b6fo3jf4`.
- **Ably JWT TTL**: 86400000ms (24h). Reconnect needed after expiry.
- **`method: "lazy"`** in push events = delivered via polling/history, not live WebSocket push.
- `GlobalPushServiceBackendChangeTopic` fires when backend switches — client must reconnect.
- Stream `streamId` format: `01KTG4EAVD8QG6VHGZCE6BXHEZ` (ULID-like).
- **`sig` field in send_tip**: passed through from caller — likely a server-issued nonce or HMAC. Origin unclear; may be optional or empty string for basic tips.
- **Token purchase**: flows through `/tipping/crypto_invoices/` (crypto) or external billing. No direct HTTP endpoint observed for card purchases — likely handled by third-party payment processor iframe.
- **Django server ID** visible in page HTML as comment: `django-{hash}-{pod}:{hash}:{hash}-{region}` (e.g. `django-57d8bc64b7-vfn8p:ca0bcf9ab708:a07f18ae3813da04-MIA`). MIA = Miami datacenter.
