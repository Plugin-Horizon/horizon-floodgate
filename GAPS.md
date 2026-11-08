# Gaps against the real Floodgate

Checked against GeyserMC's Floodgate source (MIT, `master` at 81b65cc, "Update to 26.3") and the public wiki
(floodgate/commands). The wiki lists `fwhitelist`, `linkaccount` and `unlinkaccount` with their permission nodes and
does not quote texts or defaults; the source does for the literal English lines and the config keys.

Translated player texts (`floodgate.command.*`, `floodgate.commands.*`, `floodgate.core.*`) come from GeyserMC's
translation repository, which has no licence, so they were not read; they stay the port's own wording (see Guesses).

## Missing: needs a Horizon hook or helper

- **`/fwhitelist add|a <player>` and `/fwhitelist remove|r <player>`** (permission `floodgate.command.fwhitelist`,
  operators). Not ported. Floodgate registers it on every non-proxy platform, so it applies here. `<player>` is a
  Bedrock name (the proxy's prefix is stripped, spaces become `_`, looked up at GeyserMC's XUID API) or a Floodgate
  UUID (`00000000-0000-0000-<xuid>`); the answers are `player_added`, `player_removed`, `player_already_whitelisted`,
  `player_not_whitelisted`, `invalid_username`, `user_not_found`, `api_unavailable`, `unexpected_error`.
  Horizon's mod API has no helper that adds or removes a profile in the server's whitelist (`whitelist.json`); the
  `whitelist_state_update` hook only reports on/off. Needed: a helper such as `native_whitelist_add(profile)` and
  `native_whitelist_remove(profile)` that edit the dedicated server's whitelist and report whether it changed.

## Missing: too big for this port

- **Skin upload service** (`floodgate:skin` upload to GeyserMC's websocket, lookup of a linked player's Java skin).
  `tungstenite` is now in Horizon's mod deps, but the upload protocol was not read and the TLS layer (rustls, as the
  discordsrv port does it) is port-side work: not started.
- **Own linking, MySQL and MongoDB.** SQLite own linking is done (2026-11-08, `rusqlite`; `core/src/store.rs`). The other
  database types are not ported.
- **`/floodgate firewall`** (permission `floodgate.command.floodgate.firewall`, operators; listed in `/floodgate`
  help for operators, so the help differs for them), metrics, news (`floodgate.news.receive`), the API's events and
  `getPlayerLink`'s write calls.
- **Update check of `/floodgate version`**: Floodgate asks GeyserMC's version API and answers "newer version
  available", "latest", "404" or "cannot check". The port answers only with its own version line; a blocking HTTP
  call on the server thread was not added.

## Partly done

- **Permission nodes**: each usage carries its node through `NativeCommand::permission`: `floodgate.command.floodgate`
  (default everyone), `floodgate.command.floodgate.version` (operators), `floodgate.command.linkaccount`,
  `floodgate.command.unlinkaccount` (everyone). Horizon takes the first usage's node as the command's node, so
  `floodgate version` shares `floodgate.command.floodgate` in a permissions mod's eyes. Nodes only matter with a
  permissions mod (a LuckPerms port) through the `command_permission` API; the bundle does not declare it here
  (horizon-mixin.toml has no hooks list, as the other ports do not).
- **Usage syntax**: `linkaccount <player:player_name> [code:word]` (LinkAccountCommand: `player` required, `code` optional).
  `player` is `player_name` (2026-11-08): any name is accepted, offline players included, and the server completes the
  online names (Horizon's `player_name` kind, `tab_complete_v2`).
  `unlinkaccount` takes no argument.
- **Command answers**: `/linkaccount` and `/unlinkaccount` only point to Global Linking, since own linking is missing.
  Floodgate's own answers for a Bedrock player without a code (`bedrock_usage`), a Java player with one
  (`java_usage`) and the code flow are part of own linking and are not ported.
- **Sender errors**: `/linkaccount` and `/unlinkaccount` require a player sender (`senderType(PlayerAudience)`); the console gets
  Cloud's default text, which is not known here. The port answers `This command can only be run by a player.`.

## Guesses (no public text to check against)

- The translated wording: the linking notice, `linking_disabled`, `not_a_player`, `NO_PERMISSION` (Cloud's own text
  is what Floodgate shows there), the `require-link` message (`core/src/link.rs`, `not_linked_message`).
- Config defaults in `README.md` for `[disconnect]` texts and `link-code-timeout` (300), and `type` (`sqlite`).
  The `[disconnect]` defaults and `link-code-timeout`, `type` do match the source's `config.yml`.
- `/floodgate version` at operator level 1 (`core/src/commands.rs`, `OP_LEVEL`); the source's permission default
  for it is `OP`, which is the same.
- The version line: the source prints `You're currently on <version> (branch: <branch>)` and then the update
  check (see above); the port prints its own name and the Floodgate data version.
