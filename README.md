# horizon-floodgate

> **Unofficial Rust port of Floodgate for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the Floodgate authors.**

The backend part of [Floodgate](https://github.com/GeyserMC/Floodgate) as a native mod for
[Horizon](https://github.com/arismoko/horizon) servers. Not affiliated with GeyserMC or Floodgate.

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

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

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

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` in `src/lib.rs`):

- The Floodgate handshake is not read, so a Bedrock player is not a Floodgate player yet: no key check, no forms.
- Global Linking lookups are not done (a web request would block the tick), so `require-link` is not enforced with Global Linking on.

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
