# Architecture

_Status: 2026-10-31 · revision 1_

## Layers

Lower layers never depend on higher ones (`cargo xtask layers`). Nothing below L6 touches a UI
toolkit, so the front end is swappable.

```
L0  core (ids, Ts, limits, names)          text (markup, mentions, emoji)  [standalone]
L1  model     Workspace, User, Channel, Message · Event · State (event-sourced, total apply)
L2  proto     Op · ClientFrame · ServerFrame      store   JSONL journal      search  query + rank
L3  hub       the authority: validate Op → Events, ids, permissions, visibility; demo workspace
L4  engine    Session (replica + view + drafts) · Transport (Loopback) · commands · catalog
L5  matrix    Matrix client transport, E2EE (matrix-rust-sdk)      net  own WebSocket server (legacy)      mcp  MCP
L6  ui-egui   ChatApp: panels draw from Session, push Acts, Acts run commands
    apps      chatcraft (desktop) · chatcraft-cli · chatcraft-server · chatcraft-web
```

## Transports

- **Matrix** (`crates/matrix`, the network protocol going forward): the client talks to any Matrix
  homeserver; encryption is end to end. The homeserver, not our hub, is the authority. See
  [matrix.md](matrix.md).
- **Loopback** (`engine`): the in-process hub for the demo workspace, the web build and tests.
- **ChatCraft protocol** (`net`, `chatcraft-server`): the original WebSocket protocol, kept for
  now as a minimal self-host option and as a test bed; superseded by Matrix.

## Data flow (in-process hub and ChatCraft protocol)

1. A person clicks, types a shortcut, or an agent calls a tool → `Session::run(id, params)`.
2. A mutating command builds an `Op` and `Session::submit`s it through its `Transport`.
3. The hub validates the op for that user (`hub/src/ops.rs`), assigns ids and sequence numbers,
   applies the resulting `Event`s to the canonical `State`, and returns them.
4. The server appends them to `events.jsonl` (fsync) **before** acknowledging, sends the requester
   its own visible events, then the `Ack`, and fans the events out to every other connection that
   may see them (`State::event_visible`).
5. Every client applies the events to its replica. Nothing else ever changes a replica.

`Loopback` runs the same hub in-process, so the demo workspace, the web build and most tests
exercise exactly the production logic.

## Persistence

The journal is the source of truth: one `Sequenced { seq, event }` JSON object per line. On start
the server replays it into a fresh hub. A torn last line or a corrupt line is skipped and reported;
the file is never rewritten silently. Snapshots and compaction are future work (gaps.md §8).

## Visibility and privacy

- Public channels are visible to every non-guest member; private channels and DMs only to members.
- Per-user events (read cursors, saved items, mutes, stars) go only to that user.
- Requests naming something the user can't see get "not found", so ids can't be probed.
- Joining a private conversation triggers a `Resync` (a fresh snapshot) for the joiner.

## Robustness

No panics in production code (lints deny them); `State::apply` ignores unknown references;
`Session::run` converts an escaped panic into an error; frames are capped at 4 MiB; text lengths,
reaction counts, group sizes and search results are bounded (`core::limits`). Hostile-input
sweeps cover every command and every op.
