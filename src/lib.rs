//! Floodgate's backend part as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-floodgate-core` crate (`core/`); this file only wires it to hooks and host methods.
//!
//! What works here: `/floodgate` (with `version` and `firewall`), `/linkaccount`, `/unlinkaccount` (the usage levels, and the nodes of the
//! two link commands), the `floodgate:` channels both ways (raw play channels, arismoko #3209: the tick's payloads
//! with `player.send_payload`, form answers and uploaded skins through the channel handler), the kick of
//! `Out::Kick` (`host.player_disconnect`), `require-link` for Bedrock-looking UUIDs
//! with own linking, and forgetting a player who leaves.
//!
//! What does not work yet, each one a `TODO(arismoko-api: ...)` below and listed in PORT_NOTES.md: the
//! handshake (Floodgate data in the host name is never read, so no Bedrock player is a Floodgate player,
//! no key check, no forms), Global Linking lookups (a web request would block the tick), and the core's own
//! `minecraft:register` of the channels (mods may not send on `minecraft:`; not needed by Geyser).

pub mod horizon_floodgate_core;
use fg::channels::Out;
use fg::commands::{Sender, run, usages};
use horizon_floodgate_core as fg;
use mod_api::{
    Command, CommandArgumentValue, CommandSource, Flow, Hooks, IPayloadContext, IPayloadHandler, PayloadBytes, PayloadHandleError, PayloadId, Registrar,
    RegistrationContext, PlayerCommandEvent, PlayerLoggedOutEvent, PlayerLoginEvent,
    NodeDefault, PermissionGatherNodesEvent, PermissionLevel, PermissionNode, PermissionValueType,
    RegisterCommandsEvent, RunModCommandEvent, ServerAboutToStartEvent, ServerHost, ServerTickEvent,
    ServerTickPhase, Suggestion, TabCompleteEvent, Text,
};

#[derive(Default)]
struct Mod;

/// One `floodgate:` channel: what a player's connection sends on it goes to the core.
struct Inbox(&'static str);

impl IPayloadHandler for Inbox {
    fn handle(&self, payload: PayloadBytes, context: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let player = context.player().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
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

/// The typed node for one `floodgate.*` name (a Boolean, as the commands ask it).
fn perm_node(name: &str) -> PermissionNode {
    PermissionNode::new(name, PermissionValueType::Boolean)
}

/// The answer lines to a command line (without its slash), or None when it is not Floodgate's. `sender` is
/// the player (uuid, name); None is the console or a command block.
fn answer(host: &mut dyn ServerHost, sender: Option<(u128, &str)>, line: &str) -> Option<Vec<Text>> {
    let config = fg::config();
    let sender = match sender {
        None => None,
        Some((uuid, name)) => {
            let player = host.player(uuid)?;
            let mut level = player.permission_level(&*host).unwrap_or(0);
            // Audit 2026-11-14: a granted node of a sub-command must pass the level check in the core's
            // `run` (it only compares levels), as the engine tree already does for the node.
            for node in ["floodgate.command.floodgate.version", "floodgate.command.floodgate.firewall"] {
                if player.permission_bool(&mut *host, &perm_node(node)).unwrap_or(false) {
                    level = level.max(fg::commands::OP_LEVEL);
                }
            }
            Some(Sender { level, uuid, name: name.to_string() })
        }
    };
    let lines = run(line, sender.as_ref(), &config.player_link)?;
    Some(lines.iter().map(|l| legacy_text(&*host, l)).collect())
}

/// A line without its `§x` colour codes, for the console log.
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{a7}' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn permission_gather_nodes(&self, event: &mut PermissionGatherNodesEvent<'_>) {
        // The usages' nodes, each at the operator level its usage needs (the level the string nodes fell back to).
        let config = fg::config();
        for (_, level, node) in usages(&config.player_link) {
            if let Some(node) = node {
                let _ = event.nodes.add(perm_node(node), NodeDefault::Level(PermissionLevel::from_level(i32::from(level))));
            }
        }
    }

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
                command = command.with_node(&perm_node(node));
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
        let Some(lines) = answer(host, Some((event.player, &*event.name)), event.command) else {
            return Flow::Continue;
        };
        event.replies.extend(lines);
        Flow::Cancel
    }

    /// `/floodgate`, `/linkaccount` and `/unlinkaccount` from the console, RCON or a command block (the
    /// console may run `/floodgate`; the link commands answer that they are for players), and from a player
    /// through `execute`. A player's typed line is answered in `player_command`.
    fn run_mod_command(&self, host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
        if !matches!(
            event.command.literals.first().map(String::as_str),
            Some("floodgate" | "linkaccount" | "unlinkaccount")
        ) {
            return Flow::Continue;
        }
        // The typed line again: the literal path, then the arguments (every usage has them in that order).
        let mut line = event.command.literals.join(" ");
        for argument in &event.command.arguments {
            if let CommandArgumentValue::String(text) = &argument.value {
                line.push(' ');
                line.push_str(text);
            }
        }
        let sender = match &event.source {
            CommandSource::Player { player, .. } => Some((player.uuid(), player.name(&*host).unwrap_or_default())),
            _ => None,
        };
        let Some(lines) = answer(host, sender.as_ref().map(|(u, n)| (*u, n.as_str())), &line) else {
            return Flow::Continue;
        };
        for line in lines {
            let _ = host.commands().feedback(line);
        }
        Flow::Cancel
    }

    /// The player name of `/linkaccount <player>`: the engine's `word` argument suggests nothing, so the
    /// online players are offered (Floodgate's `PlayerAudienceArgument` does).
    fn tab_complete(&self, host: &mut dyn ServerHost, event: &mut TabCompleteEvent<'_>) {
        let typed = event.command;
        let line = typed.strip_prefix('/').unwrap_or(typed);
        let words: Vec<&str> = line.split(' ').collect();
        if words.len() != 2 || !words[0].eq_ignore_ascii_case("linkaccount") || !fg::config().player_link.register_commands() {
            return;
        }
        let start = typed.len() - words[1].len();
        let prefix = words[1].to_ascii_lowercase();
        for p in host.players().server_players() {
            if let Some(name) = p.name(&*host)
                && name.to_ascii_lowercase().starts_with(&prefix)
            {
                event.suggestions.push(Suggestion::new((start, typed.len()), name));
            }
        }
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
        // The rest of `/floodgate firewall`, from its thread: to the player, or to the log for the console.
        for (to, lines) in fg::commands::take_later() {
            match to {
                Some(uuid) => {
                    if let Some(p) = players.iter().find(|p| p.uuid() == uuid) {
                        for line in &lines {
                            let text = legacy_text(&*host, line);
                            let _ = p.tell(host, text);
                        }
                    }
                }
                None => {
                    for line in &lines {
                        log("INFO", &plain(line));
                    }
                }
            }
        }
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
                        let _ = p.disconnect(host, Text::literal(reason.to_string()));
                    }
                }
                Out::ApplySkin { player, skin } => {
                    // HMOD-GAP(profile-property): Horizon main can read a player's profile but has no call to set its
                    // textures or to refresh the player. The uploaded skin is not applied; the player keeps their own skin.
                    let _ = skin;
                    log("WARN", &format!("skin of {player:032x} not applied: no profile API on Horizon main"));
                }
                Out::Log { level, line } => log(level, &line),
            }
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}
