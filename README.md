> **Frozen branch (`horizon-plain`).** This is the version written for horizon-rust/horizon's native hooks / horizon-mixins. We stopped working on it on 2026-11-12: horizon-rust moved all mod API work to the fork [arismoko/horizon](https://github.com/arismoko/horizon) (see "Mod-related pull requests" in its CONTRIBUTING.md) and closed mod pull requests there. This branch is experimental and unsupported. The maintained version, for arismoko/horizon only, is on the default branch.

# horizon-floodgate

> **Unofficial Rust port of Floodgate for the Horizon server (https://github.com/horizon-rust/horizon). Not affiliated with or endorsed by the Floodgate authors.**

The backend part of [Floodgate](https://github.com/GeyserMC/Floodgate) as a native mod for
[Horizon](https://github.com/horizon-rust/horizon) servers. Not affiliated with GeyserMC or Floodgate.

Bedrock players join a Java network through Geyser and a proxy that runs Floodgate. With
`send-floodgate-data: true` the proxy puts each Bedrock player's data, encrypted with the proxy's Floodgate key,
into the handshake it sends the backend server. This mod is what Floodgate is on a Paper backend behind such a proxy:

- It reads that data from the handshake (`<host>\0^Floodgate^...`), decrypts it with the shared key and checks it.
- It refuses the login with Floodgate's own messages when the data is wrong: another key (`disconnect.invalid-key`),
  the wrong number of fields (`disconnect.invalid-arguments-length`), another data version, or data that doesn't parse.
- It accepts handshake hosts longer than 255 characters, as Paper does behind a proxy. Without that, Horizon drops
  every login whose Floodgate data is long (a Bedrock player linked to a Java account always is).
- It keeps the Bedrock player's data (Bedrock name, XUID, device, language, input mode, linked Java account) while
  they are online, under their UUID, and logs Floodgate's line
  `Floodgate player logged in as <name> joined (UUID: <uuid>)`. Other mods ask the crate
  (`horizon_floodgate_core::is_floodgate_player(uuid)`, `player(uuid)`).

- Plugin channels (`floodgate:form`, `floodgate:packet`, `floodgate:skin`, `floodgate:transfer`), as Floodgate on a
  Paper backend: other mods send Bedrock forms (modal, simple, custom, in Cumulus 1.1's JSON) and get the answer
  back parsed (closed, invalid, or the button/values), close a player's forms, send a transfer to another Bedrock
  server or a raw packet for Geyser. A player's forms are answered as closed when they leave. Payloads from players
  are refused as Floodgate refuses them (a non-Bedrock player, a packet or transfer from the client, a malformed
  skin), with Floodgate's kick messages. Each player is sent `minecraft:register` with the four channels on join.
- Linking: a Bedrock player whose proxy sent no link is looked up at GeyserMC's Global Linking API (as Floodgate's
  `fetchLinkedPlayer`); `require-link` refuses Bedrock players without a link. As on Paper with proxy data, a link
  found on the backend goes into the player's Floodgate data only; the name and UUID stay the proxy's.
- Commands: `/linkaccount <player> [code]` and `/unlinkaccount` (with Global Linking only they point to it, as
  Floodgate does without a database), `/floodgate` (everyone; the help lists what each may use) and
  `/floodgate version` (operators). Permission nodes as Floodgate's `Permission` enum; `/fwhitelist` is not ported.
- API for other mods (`horizon_client::floodgate::...`): `player`, `is_floodgate_player`, `all_players`,
  `player_count`, `is_floodgate_id`, `create_java_player_id`, `send_form`, `close_form`, `send_transfer`,
  `send_packet`, `uploaded_skin`, `xuid_for`, `gamertag_for` (blocking calls to the Global API), and the form types in
  `forms`. A player is found by their Floodgate UUID or by their linked Java UUID, as Floodgate's `getPlayer`.

The player's name (`.` + Bedrock name, or the linked Java name) and UUID (`00000000-0000-0000-<xuid>`) come from the
proxy's Velocity modern forwarding, which Horizon already reads. The mod never changes them, as Floodgate doesn't on a
backend with proxy data.

## Config

`config/floodgate/config.toml` in the server folder, written with the defaults on the first login when missing.
Keys as in Floodgate's `config.yml`:

| Key | Default | |
|---|---|---|
| `key-file-name` | `key.pem` | the key file in `config/floodgate/`: the proxy's key, byte for byte |
| `username-prefix` | `.` | as on the proxy: shapes the name this mod reports |
| `replace-spaces` | `true` | as on the proxy |
| `[disconnect] invalid-key` | `Please connect through the official Geyser` | |
| `[disconnect] invalid-arguments-length` | `Expected {} arguments, got {}. Is Geyser up-to-date?` | sent as written, as Floodgate does |
| `[player-link] enabled` | `true` | off: nothing is looked up and the link commands are not registered |
| `[player-link] require-link` | `false` | refuse Bedrock players without a linked Java account |
| `[player-link] enable-global-linking` | `true` | look up links at `api.geysermc.org` |
| `[player-link] enable-own-linking`, `allowed`, `link-code-timeout`, `type` | `false`, `true`, `300`, `sqlite` | own linking uses SQLite (`config/floodgate/linked-players.db`, `type = "mysql"` is not ported and also uses SQLite); `link-code-timeout` seconds for a link request |

Keys of Floodgate's `config.yml` that this port does not read: `default-locale` (the texts are English only),
`metrics` (no telemetry is sent) and `config-version`.

Without a usable key every Bedrock login is refused with `invalid-key` and the log says why; Java players are not
affected. The key is read once: restart the server after changing it. Unlike Floodgate, the mod never generates a key
(a backend's key must be the proxy's anyway).

## Build

A [horizon-mixins](https://github.com/horizon-rust/horizon) bundle: this folder is the bundle (`horizon-mixin.toml`,
`helpers/`, the crate `core/`). Put it in a mods folder and build Horizon with it:

```sh
horizon-mixins apply --workspace <horizon source> --mods <mods folder> --out <stage>
cd <stage> && cargo build --profile fast -p horizon-client --no-default-features
cd <stage> && cargo test -p horizon-floodgate-core            # the crate's tests
```

Tested on Horizon `69fab597a6f0e8a6ce524d38d1b73e599e343034` (native hooks API 411). The crate uses only `ring`,
`base64`, `serde` and `toml` from Horizon's `Cargo.lock`.

## How it hooks in

All but one injection use the hooks of Horizon's `hooks-all` branch (`fce1fb23d`, not merged upstream yet):

| Injection | Target | Why |
|---|---|---|
| `handshake` (HEAD) | hooks-all `lan::conn::native_handshake` | decrypts the host's Floodgate data, puts the plain hostname back, refuses wrong data with Floodgate's message (no lookup here: the hook must not block) |
| `long-host` (HEAD, cancellable, raw) | `buf::Reader::string` | without forwarding (Geyser straight to this server), reads the handshake host with 32767 instead of 255; hooks-all already does that with forwarding on |
| `pre-login` (HEAD) | hooks-all `lan::conn::native_player_pre_login` | Global Linking lookup and `require-link` (a refusal goes back on `defer`) |
| `profile` (HEAD) | hooks-all `lan::conn::native_player_profile` | the admitted Bedrock player becomes a Floodgate player |
| `payload` (HEAD) | hooks-all `native_custom_payload_from_client` | a player's `floodgate:*` payload |
| `tick` (HEAD) | `native_server_tick` | twice a tick: `minecraft:register` and queued payloads (`native_send_custom_payload_to_player`), uploaded skins (`native_set_profile_property` + `native_refresh_player`), kicks (vanilla `/kick`) |
| `logged-out` | hook point `player_logged_out` v1 | forgets the player, closes their forms |
| `register`, `perform` | hook points `register_commands` v1, `perform_command` v1 | the commands |
| module `api` | `horizon_client::floodgate` | the crate's API for the game's code (the crate itself is a dependency of `horizon-client-lib`) |

The handshake, pre-login and profile hooks run on the same connection thread, in that order, so the outcome is
handed over in a thread-local. `on_connection`/`peek_handshake` are no longer called by the glue and stay only for
their tests.

## Tests

- `cargo test -p horizon-floodgate-core`: the cipher, the data format, the checks and the handshake reader, plus
  `tests/interop.rs`: data that GeyserMC's own classes (Floodgate 2.2.5) encrypted, with a throwaway test key
  (`core/tests/test-key.bin`, protects nothing). `tools/Interop.java` made it, and reads what the crate encrypts.
- `tools/probe.py`: synthetic Bedrock logins, as Geyser into a Velocity proxy with Floodgate, or as a Floodgate proxy
  (with Velocity modern forwarding signed by a test secret) straight into the server.

## Not done or untested

The plugin channels, forms, linking, commands and API calls (branch `b-features`) were written without a build of
the engine and without a server: only the crate's unit tests ran. The glue (`helpers/*.rs`) has not been compiled.

- Skins: a skin Geyser uploads after the join (`floodgate:skin`) is kept (`uploaded_skin`) and applied to the
  online player with hooks-all's `native_set_profile_property` + `native_refresh_player` (untested). Floodgate's skin upload service (a TLS websocket to GeyserMC) and the lookup of a
  linked player's Java skin are not ported. The skin the proxy forwards at login comes through Velocity forwarding.
- Own linking (`enable-own-linking`): SQLite only (`rusqlite`, Horizon's `sqlite` feature), written 2026-11-08 and tested
  on its own (`store.rs`, `commands.rs`): a Java player runs `/linkaccount <bedrock name>` and gets a four-digit code, the
  Bedrock player runs `/linkaccount <java name> <code>` and is disconnected to join again as the linked account;
  `/unlinkaccount` removes it. A login looks in the database first, then at Global Linking. MySQL and MongoDB are not
  ported. The table layout and all texts are guesses.
- `/fwhitelist` (needs a helper that adds a profile to the whitelist), `/floodgate firewall`, metrics, news, the
  update check, the API's events and `getPlayerLink`'s write calls.
- Texts: Floodgate's translated texts (`floodgate.core.not_linked`, the command answers) come from GeyserMC's
  translation repository, which has no licence, so they were not read; this port's own English wording is used.
  Players are not answered in their own language.
- Forms: Gson escapes `<`, `>`, `&`, `=` and `'` in the JSON as `\u003c` and so on; this port writes them as they
  are (the same JSON value). Not tried with a Bedrock client.
- Floodgate also sets the connection's address to the Bedrock client's address; behind Velocity forwarding the
  forwarded address wins on Paper too, so the mod leaves it.
- Tested with a real Velocity 4.3.1 with Floodgate 2.2.5 and synthetic clients only, not with Geyser and a Bedrock
  client.

## Licence

Derived from [Floodgate](https://github.com/GeyserMC/Floodgate) (https://github.com/GeyserMC/Floodgate, MIT).

NOTICE (original copyright holders): Copyright (c) 2019-2022 GeyserMC (Floodgate and the Floodgate classes in
Geyser's `common` module; the MIT notice is reproduced in LICENSE). The port's own text: Copyright (c) 2026 Detimes.

MIT, see [LICENSE](LICENSE). Floodgate and the Floodgate classes in Geyser's `common` module are MIT (checked
2026-11-06), so their source was read: the data format, the cipher layout and the order of the checks follow
`AesCipher`, `Base64Topping`, `FloodgateCipher`, `BedrockData`, `LinkedPlayer`, `FloodgateHandshakeHandler`,
`CommonDataHandler` and `HandshakeDataImpl`; GeyserMC's notice is in LICENSE. No clean room was needed.

For the plugin channels, linking and commands (2026-11-06) these were read as well, from Floodgate's `master`:
`FormChannel`, `PacketChannel`, `SkinChannel`, `TransferChannel`, `PluginMessageChannel`,
`SpigotPluginMessageRegistration`, `SpigotPluginMessageUtils`, `SimpleFloodgateApi`, `PlayerLinkHolder`,
`GlobalPlayerLinking`, `CommonPlayerLink`, `HttpClient`, `LinkAccountCommand`, `UnlinkAccountCommand`,
`CommonCommandMessage`, `MainCommand`, `VersionSubcommand`, `Permission`, `DeviceOs`, `InputMode`, `UiProfile` and
`config.yml`; and from GeyserMC's Cumulus 1.1.2 (MIT, the version Floodgate uses, commit `496d5572`): the form types,
the three codecs, `FormCodecImpl`, `FormImageAdaptor` and the component classes. GeyserMC's translation repository
(`GeyserMC/languages`) has no licence and was not read.

## Contributing

Pull requests and improvements are always welcome! Open an issue or a PR -- direct pushes to main are disabled, every change goes through review.
