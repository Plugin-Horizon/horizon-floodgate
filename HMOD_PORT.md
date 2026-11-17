# hmod port: floodgate
Round 1
- Fixed: `IPayloadHandler::handle` returns `PayloadHandleError` (`Inbox`, src/lib.rs).
- Fixed: `Command::with_node` takes `&PermissionNode`; built with `perm_node(node)` (`register_commands`, src/lib.rs).
- Fixed: `permission_value == Some(true)` replaced by `permission_bool(...).unwrap_or(false)` (`player_command`, src/lib.rs).
- Fixed: `Out::Kick` uses `host.player_disconnect(player, reason)` (the host call that disconnects a player; `PlayerRef::kick` does not exist) (`server_tick`, src/lib.rs).
- Gap (see below): `Out::ApplySkin` no longer calls `set_profile_property` / `refresh`; it logs a warning instead (`server_tick`, src/lib.rs).
- Added: `permission_gather_nodes` declares each node from `usages(&config.player_link)` at the usage's level (src/lib.rs).
- Permission nodes: `floodgate.command.floodgate` -> default level 0 (everyone); `floodgate.command.floodgate.version` -> level 1 (`OP_LEVEL`); `floodgate.command.linkaccount` and `floodgate.command.unlinkaccount` -> level 0. All declared in `permission_gather_nodes` (src/lib.rs). The version node's default equals the old level check, so level-0 players get nothing new.
- Gaps:
  - `profile-property`: Horizon main has no call to set a player's profile textures or to refresh the player. The uploaded Geyser skin is not applied; the warning line names the player. Marked at `Out::ApplySkin` (src/lib.rs).
- Unsure:
  - The gather hook calls `fg::config()` to read the usage nodes (the same call `register_commands` makes).
  - `player_disconnect` may differ from a plain kick in its broadcast or its reason text; the reason is passed as before.
  - Module header updated: it no longer claims the skin path works.
