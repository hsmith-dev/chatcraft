# Matrix and end-to-end encryption

_Status: 2026-11-01 · revision 1_

ChatCraft speaks **Matrix**, the open, federated chat standard, instead of a protocol of its own
(the direction proposed in storytold/artcraft#2138). Any Matrix homeserver works — Synapse,
Conduit/continuwuity/tuwunel, or a hosted one — and ChatCraft interoperates with other Matrix
clients such as Element.

```sh
chatcraft --matrix https://matrix.example.org --user alice           # desktop
chatcraft-cli run --matrix https://matrix.example.org --as alice \
  --cmd 'channel.create={"name":"launch","private":true,"members":["@bob:example.org"]}'
```

The password (`--password` or `CHATCRAFT_PASSWORD`) is needed only the first time on a device.

## How it fits

`crates/matrix` provides `MatrixTransport`, an implementation of the engine's `Transport`. It runs
`matrix-rust-sdk` (Apache-2.0) on a background thread and translates both ways, so commands, the
UI, the CLI and MCP are unchanged:

| ChatCraft | Matrix |
|---|---|
| Public channel | Room with `join_rule: public`, published in the room directory, not encrypted |
| Private channel | Invite-only room, **encrypted** (`m.room.encryption`, Megolm) |
| Direct / group message | Room in `m.direct`, **encrypted** (trusted private chat) |
| Message, thread reply | `m.room.message`; threads via `m.thread` relations |
| Edit, delete | `m.replace` (accepted only from the original author), redaction |
| Reaction | `m.reaction` annotation (Unicode key ↔ our shortcodes) |
| Mentions | `m.mentions` (intentional mentions) plus `@name` in the text |
| Read state | `m.fully_read` + public read receipts |
| Star | `m.favourite` tag |
| Presence, status | `m.presence`, `status_msg` (`:emoji: text`) |
| Invite to a channel | Invites are accepted automatically (people added to a channel are in it) |

Ids are interned: Matrix's `@user:server`, `!room:server` and `$event` become ChatCraft's numeric
ids inside each client; handles are the localpart (`alice`), or `alice:other.org` when two
servers share a localpart. People not met yet can be invited by Matrix ID (`@bob:example.org`).

## Encryption

- **Olm/Megolm through the SDK** (`vodozemac`). Messages, reactions, edits and thread replies in
  encrypted conversations are encrypted on the sender's device; the homeserver stores ciphertext
  only. The integration test checks the server's copy contains no plaintext.
- **One device per store.** The device's identity and room keys live in an encrypted SQLite store
  at `<data>/ChatCraft/matrix/<user>@<server>/store`, protected by a random passphrase from the OS
  CSPRNG (`passphrase`, mode 0600); the session token is in `session.json` (0600). Restarting
  reuses the same device, so earlier messages stay readable.
- **Undecryptable messages are shown, not hidden**: "Can't decrypt this message yet" when this
  device has no key (it joined later, or is a new device without key backup).
- **Encryption is visible**: a shield marks encrypted conversations in the header and composer.

### Not done yet (see gaps.md)

- **Device verification** (emoji/SAS, QR) and **cross-signing**: today keys are shared with every
  device of the people in the room without verifying it (the SDK's default trust policy).
- **Key backup and recovery** (server-side, encrypted with a recovery key), so a new device can
  read history. Until then a new device can't read older encrypted messages.
- Storing the store passphrase and access token in the OS keychain instead of files.
- Re-trying decryption when a key arrives late; rendering `formatted_body` HTML from other
  clients; room directory browsing; pins, saved items, muting, archiving over Matrix.
- Encrypted public channels are possible in Matrix but not offered: anyone may join them, so
  encryption adds little and breaks history for newcomers.

## Testing

`crates/matrix/src/mapper/tests.rs` covers the mapping with JSON fixtures (no server). The
end-to-end test runs against a real homeserver and is skipped unless configured. With a local
Synapse (Python 3.10+):

```sh
python3 -m venv synapse && synapse/bin/pip install matrix-synapse
mkdir hs && cd hs
../synapse/bin/python -m synapse.app.homeserver --server-name localhost --config-path hs.yaml \
  --generate-config --report-stats=no
../synapse/bin/python -m synapse.app.homeserver --config-path hs.yaml &
../synapse/bin/register_new_matrix_user -u alice -p pw-alice -c hs.yaml --no-admin http://localhost:8008
../synapse/bin/register_new_matrix_user -u bob -p pw-bob -c hs.yaml --no-admin http://localhost:8008
cd .. && CHATCRAFT_TEST_HOMESERVER=http://localhost:8008 CHATCRAFT_TEST_ALICE=alice:pw-alice \
  CHATCRAFT_TEST_BOB=bob:pw-bob cargo test -p chatcraft-matrix --test e2ee -- --nocapture
```

It checks: a private channel is created encrypted; Bob (invited by Matrix ID) joins and decrypts;
reactions, thread replies and edits arrive decrypted; the homeserver's stored events contain no
plaintext; and after a restart Bob's device restores from its store and still reads the history.
Synapse runs only as an external test process; ChatCraft contains no Synapse code.

`cargo run -p chatcraft-matrix --example rooms -- <homeserver> <user> <store-dir>` prints what a
store believes about an account's rooms (diagnostics).
