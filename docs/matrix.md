# Matrix and end-to-end encryption

_Status: 2026-11-01 · revision 2 (verification, key backup)_

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

### Verifying people (emoji comparison)

- **Cross-signing** is set up on an account's first sign-in with a password, so each person has
  one identity that vouches for all their devices.
- **Verify** (profile pane, or `security.verify`) starts a Matrix SAS verification in your DM with
  them. Both of you see the same seven emoji with names; you compare them out of band and press
  *They match* (or *They don't match*, which cancels on both sides). Incoming requests open the
  dialog automatically. Verified people carry a shield in profiles and member lists, and trust is
  remembered across restarts. The integration test asserts both clients compute identical emoji.
- The emoji's name is always shown; the four the bundled font can't draw (lion, unicorn,
  butterfly, robot) show the name only, never a blank box (`ui-egui/src/sas.rs`).

### Key backup and recovery

- **Set up key backup** (profile menu › Encryption and key backup, or `security.setupRecovery`)
  creates an encrypted server-side backup of room keys and a 12-word **recovery key** shown once.
  New keys are backed up automatically from then on.
- On a new device, **Restore** with the recovery key (`security.recover`): keys are downloaded
  from the backup and messages shown as undecryptable are decrypted in place (same message,
  replies and reactions kept). A wrong key is refused. A missing key is also looked up in the
  backup automatically, and messages are re-tried whenever new keys arrive.

### Not done yet (see gaps.md)

- Verifying **your own** devices (and QR-code verification); a stricter trust policy (only send
  keys to verified devices) as an option; warnings when a verified person's identity changes.
- Storing the store passphrase and access token in the OS keychain instead of files.
- Rendering `formatted_body` HTML from other clients; room directory browsing; pins, saved
  items, muting, archiving over Matrix.
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

`crates/matrix/tests/verify.rs` needs a homeserver with open registration (test servers only:
`enable_registration: true` and `enable_registration_without_verification: true` in
`hs.yaml`) and `CHATCRAFT_TEST_REGISTER=1`; each run registers fresh accounts. It covers emoji
verification both ways (identical emoji, confirm, mismatch, trust after restart) and key backup
(a new device restores history with the recovery key; a wrong key is refused).

`e2ee.rs` checks: a private channel is created encrypted; Bob (invited by Matrix ID) joins and decrypts;
reactions, thread replies and edits arrive decrypted; the homeserver's stored events contain no
plaintext; and after a restart Bob's device restores from its store and still reads the history.
Synapse runs only as an external test process; ChatCraft contains no Synapse code.

`cargo run -p chatcraft-matrix --example rooms -- <homeserver> <user> <store-dir>` prints what a
store believes about an account's rooms (diagnostics).
