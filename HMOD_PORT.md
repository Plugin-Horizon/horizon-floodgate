# hmod port: floodgate
Parity: 72% now / 86% with our PRs / 91% with engine changes / 77% with new SDK (31/43, 37/43, 39/43, 33/43; method: 7 commands + 7 nodes + 17 config keys + 12 features; blocked-by-API counts as not done)
Round 100 (Haiku, blind, not compiled): the four values are unchanged because the count method counts items, and this round only closed behaviour inside items already counted (see the checklist below).

Checklist round 100 (original = GeyserMC/Floodgate 81b65cc, MIT, read; `languages` repo not read)
- [x] `/floodgate` help: sub-commands the sender may use (ops: firewall, version); console path.
- [x] `/floodgate firewall`: "Testing global api...", "Was able to connect...", "The checks have finished. n/1" (thread).
- [~] `/floodgate firewall` failure: port prints the reason, not Java's stack trace (no stack traces in Horizon).
- [ ] `/floodgate version`: original checks GeyserMC's latest build; port reports its own version. Not applicable (different product, no build feed).
- [x] `/linkaccount` and `/unlinkaccount`: notice order (global-only stops, both on prints the own-database notice first).
- [x] `allowed = false` (own linking): both link commands answer "linking disabled" (round 100, commands.rs).
- [x] Link code checked before expiry; a wrong code keeps the request (store.rs, as SqliteDatabase).
- [x] `/fwhitelist add|a|remove|r` with names (Global API on a thread) and UUIDs; edits `whitelist.json`.
- [x] Permission nodes: floodgate, .version, .firewall, .linkaccount, .unlinkaccount, .fwhitelist, news.receive (defaults as Permission.java).
- [x] Config keys and defaults: key-file-name, username-prefix, replace-spaces, disconnect.*, player-link.* (default `enable-global-linking = true`).
- [x] `username-prefix` of 16 or more characters becomes `.` (FloodgateConfig.postInitialize, round 100).
- [x] Prefix warnings at start (PostEnableMessages texts; logged at the first config read, not after 5 s).
- [x] `type = "mysql"` (or other non-sqlite) with own linking: warning at start; still SQLite (not ported).
- [ ] `default-locale`, `metrics`, `config-version`: not read (translations, telemetry and config versioning are not in the port).
- [ ] `floodgate-test` command: only with the original's DEBUG_MODE, which is false in release. Not ported.
- [~] Tab completion: `linkaccount <player>` suggests online players; `fwhitelist add|remove <player>` suggests online Bedrock players; sub-command literals by the engine.
- [ ] Translations: every message is the port's English (the translation repo has no licence and was not opened).
- [ ] Skin apply (`floodgate:skin` result): API-blocked (GAPS_UPSTREAM `set_profile_property`).
- [ ] Handshake data, Global Linking at login, `require-link` with Global Linking: code is behind `horizon_handshake_hook` (API-blocked, GAPS_UPSTREAM `player_handshake`).
- [ ] `minecraft:register` message: not sent (engine refuses mods on `minecraft:`); README corrected.
- [ ] Plugin channels (form, packet, skin, transfer): not re-walked in round 100; last checked in round 3.
- [ ] Own linking with type mysql/mongo: not ported.
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
