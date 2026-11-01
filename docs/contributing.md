# Contributing

1. Read [AGENTS.md](../AGENTS.md) — the clean-room and asset rules are not optional.
2. `cargo xtask ci` before every commit. Scope day-to-day work: `cargo test -p chatcraft-hub`.
3. New behaviour: Op → hub planning → Event/`State::apply` → engine command → UI, each with tests.
   Add the feature to `crates/engine/src/catalog.rs` if Slack or Teams has it, run
   `cargo xtask parity`, and raise `parity_floor`.
4. UI changes: render with the `shot` example and look at the PNG, light and dark.
5. Branch `feat/…`/`fix/…`/`docs/…`, one task id per commit, PR into `main`.

## Planning with BMad (optional)

Planning artifacts live in `plan/bmad/` (local, gitignored). To use the BMad agents:

```sh
npx bmad-method@6.12.1 install --directory . --modules bmm --tools claude-code \
  --output-folder plan/bmad --yes
```

Then e.g. `/bmad-help`, `/bmad-sprint-planning`, `/bmad-build` in Claude Code.
