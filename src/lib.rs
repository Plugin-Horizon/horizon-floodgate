//! Floodgate's backend part as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-floodgate-core` crate (`core/`); this file only wires it to hooks and host methods.
//!
//! What works here: `/floodgate`, `/linkaccount`, `/unlinkaccount` (the usage levels, and the nodes of the
//! two link commands), the `floodgate:` channels both ways (raw play channels, arismoko #3209: the tick's payloads
//! with `player.send_payload`, form answers and uploaded skins through the channel handler), the kick of
//! `Out::Kick` (`player.kick`), the skin of `Out::ApplySkin` (`player.set_profile_property` + `player.refresh`;
//! it fires once Geyser uploads a skin), `require-link` for Bedrock-looking UUIDs
//! with own linking, and forgetting a player who leaves.
//!
//! What does not work yet, each one a `TODO(arismoko-api: ...)` below and listed in PORT_NOTES.md: the
//! handshake (Floodgate data in the host name is never read, so no Bedrock player is a Floodgate player,
//! no key check, no forms), Global Linking lookups (a web request would block the tick), and the core's own
//! `minecraft:register` of the channels (mods may not send on `minecraft:`; not needed by Geyser).
use fg::channels::Out;
use fg::commands::{Sender, run, usages};
use horizon_floodgate_core as fg;
use mod_api::{
    Command, Flow, Hooks, IPayloadContext, IPayloadHandler, PayloadBytes, PayloadId, Registrar,
    RegistrationContext, PlayerCommandEvent, PlayerLoggedOutEvent, PlayerLoginEvent,
    RegisterCommandsEvent, ServerAboutToStartEvent, ServerHost, ServerTickEvent, ServerTickPhase,
    Text,
};

#[derive(Default)]
struct Mod;

/// One `floodgate:` channel: what a player's connection sends on it goes to the core.
struct Inbox(&'static str);

impl IPayloadHandler for Inbox {
    fn handle(&self, payload: PayloadBytes, context: &mut dyn IPayloadContext) -> Result<(), String> {
        let player = context.player().map_err(|e| format!("{e:?}"))?;
        fg::channels::on_payload(player.uuid(), self.0, &payload.0);
        Ok(())
    }
}

fn log(level: &str, line: &str) {
    eprintln!("[{level}] [floodgate] {line}");
}

/// A reply in legacy colour codes (`§`), read as the engine draws them (`host.text_from_legacy`, as the
/// SkinsRestorer port does). A line the host refuses keeps its words, codes included.
fn legacy_text(host: &dyn ServerHost, line: &str) -> Text {
    host.text_from_legacy(line)
        .unwrap_or_else(|_| Text::literal(line.to_owned()))
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    /// The four Floodgate channels (form, packet, skin, transfer) as raw play channels, each with the inbox.
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        let mut payloads = registrar.payload_handlers();
        let mut raw = payloads.registrar("1");
        for channel in fg::channels::CHANNELS {
            raw.play_raw(PayloadId(channel.into()), Some(std::sync::Arc::new(Inbox(channel))));
        }
    }

    fn server_about_to_start(&self, _: &mut dyn ServerHost, _: &mut ServerAboutToStartEvent) {
        // Reads config/floodgate/config.toml (written from the default when missing) and the key once,
        // and logs what is missing. The key itself is never printed.
        let config = fg::config();
        log(
            "INFO",
            &format!(
                "config read: global linking {}, own linking {}, require-link {}",
                on_off(matches!(
                    config.player_link.linking(),
                    fg::link::Linking::Global
                )),
                on_off(config.player_link.own()),
                on_off(config.player_link.require_link),
            ),
        );
        // TODO(arismoko-api: handshake): the Floodgate data after the host's NUL is never read, and the host
        // name is not cut back to the plain one (Floodgate's `separate_hostname`).
        log(
            "WARN",
            "Bedrock login data is not read on arismoko yet (no handshake hook): no Bedrock player is a Floodgate player",
        );
    }

    fn register_commands(&self, _: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        let config = fg::config();
        for (usage, level, node) in usages(&config.player_link) {
            // The player name is a `word` here: arismoko has no `player_name` argument kind, and the
            // core's `run` reads the name from the text anyway.
            let usage = usage.replace("<player:player_name>", "<player:word>");
            let mut command = if level == 0 {
                Command::new(&usage)
            } else {
                Command::op(&usage, level)
            };
            // The usage's Java node gates its literal path (the provider's answer, else the level):
            // `/floodgate` (everyone) and `/floodgate version` (operators) each keep their own.
            if let Some(node) = node {
                command = command.with_node(node);
            }
            event.commands.push(command);
        }
    }

    /// Floodgate's commands (`perform_command` in the Horizon port). The line is answered here, before
    /// the engine runs it, and the reply goes to the player as system chat.
    fn player_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut PlayerCommandEvent<'_>,
    ) -> Flow {
        let config = fg::config();
        let Some(player) = host
            .players()
            .server_players()
            .into_iter()
            .find(|p| p.uuid() == event.player)
        else {
            return Flow::Continue;
        };
        let mut level = player.permission_level(&*host).unwrap_or(0);
        // Audit 2026-11-14: a granted `floodgate.command.floodgate.version` node must pass the level check
        // in the core's `run` (it only compares levels), as the engine tree already does for the node.
        if player.permission_value(&*host, "floodgate.command.floodgate.version") == Some(true) {
            level = level.max(fg::commands::OP_LEVEL);
        }
        let sender = Sender {
            level,
            uuid: event.player,
            name: event.name.to_string(),
        };
        if let Some(lines) = run(event.command, Some(&sender), &config.player_link) {
            event.replies.extend(lines.iter().map(|line| legacy_text(&*host, line)));
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// `require-link` (Floodgate's `on_pre_login` in the Horizon port). Only the own-database lookup runs
    /// here. TODO(arismoko-api: player_pre_login): a Global Linking lookup is a web request and would stall
    /// the server thread, so with Global Linking on, `require-link` is not enforced yet. Also: this hook
    /// only sees Bedrock-looking UUIDs, not the handshake's data.
    fn player_login(&self, _: &mut dyn ServerHost, event: &mut PlayerLoginEvent<'_>) -> Flow {
        let config = fg::config();
        let link = &config.player_link;
        if !link.require_link
            || matches!(link.linking(), fg::link::Linking::Global)
            || !fg::is_floodgate_id(event.player)
        {
            return Flow::Continue;
        }
        // A Floodgate UUID is 00000000-0000-0000-<xuid>: the xuid is its low 64 bits.
        let xuid = (event.player as u64) as i64;
        let linked = match fg::find_link(link, xuid) {
            Ok(found) => found.is_some(),
            Err(line) => {
                log("ERROR", &line);
                false
            }
        };
        if !linked {
            *event.reason = Some(Text::literal(fg::link::not_linked_message()));
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn player_logged_out(&self, _: &mut dyn ServerHost, event: &mut PlayerLoggedOutEvent<'_>) {
        fg::remove_player(event.player);
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        if !matches!(
            event.phase,
            ServerTickPhase::ServerTickStart {
                world_running: true,
                ..
            }
        ) {
            return;
        }
        let players = host.players().server_players();
        let online: Vec<u128> = players.iter().map(|p| p.uuid()).collect();
        for out in fg::channels::flush(&online) {
            match out {
                Out::Payload {
                    player,
                    channel,
                    data,
                } => {
                    // `minecraft:register` is the engine's, refused for mods; the channels are still declared.
                    if channel.starts_with("minecraft:") {
                        continue;
                    }
                    if let Some(p) = players.iter().find(|p| p.uuid() == player) {
                        if let Err(e) = p.send_payload(host, &PayloadId(channel.into()), PayloadBytes(data)) {
                            log("WARN", &format!("{channel}: {e:?}"));
                        }
                    }
                }
                Out::Kick { player, reason } => {
                    if let Some(p) = players.iter().find(|p| p.uuid() == player) {
                        let _ = p.kick(host, Text::literal(reason.to_string()));
                    }
                }
                Out::ApplySkin { player, skin } => {
                    // The uploaded skin becomes the profile's textures, then the player is shown again.
                    match players.iter().find(|p| p.uuid() == player) {
                        Some(p) => {
                            if p.set_profile_property(
                                host,
                                "textures",
                                skin.value.as_str(),
                                Some(skin.signature.as_str()),
                            ) {
                                let _ = p.refresh(host);
                            } else {
                                log("WARN", &format!("skin of {player:032x} refused by the profile API"));
                            }
                        }
                        None => log("WARN", &format!("skin of {player:032x}: player not online")),
                    }
                }
                Out::Log { level, line } => log(level, &line),
            }
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}
