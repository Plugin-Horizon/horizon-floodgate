# hmod port: floodgate
Parity: before 60% -> after 72% default build, 86% with feature horizon_handshake_hook (31/43, 37/43; method: 7 commands + 7 nodes + 17 config keys + 12 features; blocked-by-API counts as not done)
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

Round 2 (parity fixes)
- Fixed: default config had `enable-global-linking = false` (now `true`, as Floodgate and the README), so the link commands are registered on a fresh install; `/floodgate`, `/linkaccount`, `/unlinkaccount` now work from the console, RCON and command blocks (`run_mod_command`); `/floodgate firewall` added (node `floodgate.command.floodgate.firewall`, operators, health request on its own thread, answers delivered next tick); help lists `firewall`; `/linkaccount <player>` suggests online players (`tab_complete`); README and `horizon-mod.toml` description say honestly that the handshake is not read.
- Gaps: handshake / Floodgate data (needs an engine hook), `/fwhitelist` (no whitelist API), skin application, `require-link` with Global Linking, `floodgate.news.receive`.
- Dropped finding: `OP_LEVEL = 1` is kept (Bukkit's `isOp()` is any ops entry, which is level 1 and up).

Round 3 (parity, written blind, not compiled)
- Added: `whitelist.rs` and `/fwhitelist add|a|remove|r` (edits `whitelist.json`, tick runs `whitelist reload`; name lookup on a thread), node `floodgate.news.receive`, local-linking notice for both link commands, `JAVA_USAGE`, `fwhitelist` tab completion.
- Handshake: `on_handshake_hook` / `on_login` in the core (pending logins keyed by the forwarded UUID, link lookup on a thread, pruned after 60 s) and `player_handshake` + `player_login` wiring in `src/lib.rs` behind cargo feature `horizon_handshake_hook` (off: the SDK pin lacks the hook; PlayerHandshakeEvent field names `host: &mut String`, `reason`, are inferred from the hook declaration). Core tests cover it.
- Open: skin apply, translations, whitelist API (GAPS_UPSTREAM.md).
