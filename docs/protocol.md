# Wire protocol (v1)

_Status: 2026-10-31 · revision 1_

WebSocket text frames, one JSON object each, tagged by `type`. Source of truth: `crates/proto/src/lib.rs`.
Ids are integers on the wire and accept `"12"` or `"U12"`/`"C12"`/`"M12"` on input.

## Client → server
| Frame | Fields |
|---|---|
| `hello` | `version` (1), `handle`, `displayName`?, `token` — must be first |
| `op` | `req` (client-chosen number), `op` (below) |
| `ping` | `n` |

Ops (`op` tag): `postMessage`, `editMessage`, `deleteMessage`, `react`, `pin`, `createChannel`,
`joinChannel`, `leaveChannel`, `invite`, `remove`, `renameChannel`, `setTopic`, `setPurpose`,
`archive`, `openDm`, `setProfile`, `setStatus`, `setPresence`, `markRead`, `save`, `mute`, `star`,
`renameWorkspace`.

## Server → client
| Frame | Meaning |
|---|---|
| `welcome` | `me`, `state` — everything this user may see |
| `event` | `seq`, `event` — apply in order |
| `resync` | `state` — replace the replica |
| `ack` | `req`, `ok`, `result`, `error` — sent after the requester's own events |
| `error` | `message` (wrong token, wrong version, malformed frame) |
| `pong` | `n` |

Example:

```json
{"type":"hello","version":1,"handle":"ada","token":"…"}
{"type":"op","req":7,"op":{"op":"postMessage","channel":1,"text":"hi @bo"}}
← {"type":"event","seq":42,"event":{"type":"messagePosted","message":{…}}}
← {"type":"ack","req":7,"ok":true,"result":{"message":19,"channel":1}}
```
