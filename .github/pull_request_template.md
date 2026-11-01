## What and why

<!-- Task id (e.g. M1.2) and the gap or issue it closes: Fixes #… -->

## Checklist

- [ ] `cargo xtask ci` passes
- [ ] New behaviour is a command with tests; hostile-input sweeps still pass
- [ ] Catalog/parity updated if this adds a Slack or Teams feature (`cargo xtask parity`)
- [ ] UI change: looked at the `shot` render in light and dark
- [ ] No vendor assets; any new asset has an ATTRIBUTION.md row
- [ ] Clean room: nothing copied from Slack/Teams clients, private APIs or GPL code
