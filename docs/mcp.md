# MCP server

`chatcraft-cli mcp` serves the Model Context Protocol over stdio (JSON-RPC, one message per line).

- **Tools:** every engine command, named with `_` for `.` (`message_send`, `search_messages`,
  `channel_create`…), plus `list_commands`. Read-only commands carry `readOnlyHint`.
- **Targets:** `--demo [--as handle]` (in-process demo workspace) or
  `--server ws://host:port --as handle --token TOKEN` (acts as that person; give bots their own handle).
- **Conventions:** conversations are `"#name"` or `"@handle"` (DM), messages `12` or `"M12"`.

Claude Code example (`.mcp.json`):

```json
{ "mcpServers": { "chatcraft": { "command": "chatcraft-cli", "args": ["mcp", "--demo"] } } }
```
