//! Floodgate's commands on a backend: `/linkaccount`, `/unlinkaccount` and `/floodgate` (GeyserMC's
//! `LinkAccountCommand`, `UnlinkAccountCommand`, `MainCommand`, `VersionSubcommand` and `Permission`, MIT, see
//! LICENSE). Floodgate's player-facing texts live in its translation files, which have no licence, so the
//! wording of the translated lines below is the port's own; the structure and the decisions are Floodgate's.
//!
//! Not ported: `/fwhitelist` (Horizon has no helper to add a profile to the whitelist; see the README) and
//! `/floodgate firewall`.

use std::time::{Duration, Instant};

use crate::link::{LinkConfig, Linking, LINK_INFO_URL};
use crate::store::{self, LinkError, Own};

/// `floodgate.commands.global_linking_notice`.
pub fn global_linking_notice() -> String {
    format!(
        "This server uses Global Linking. Link your Java and Bedrock accounts at {LINK_INFO_URL}"
    )
}
/// `floodgate.commands.linking_disabled`.
pub const LINKING_DISABLED: &str = "Account linking is disabled on this server.";
/// `floodgate.commands.not_a_player`.
pub const NOT_A_PLAYER: &str = "This command can only be run by a player.";
/// The answer to a command the sender's level does not allow.
pub const NO_PERMISSION: &str = "You don't have permission to run this command.";

/// Who runs a command: `None` for the console.
pub struct Sender {
    pub level: u8,
    pub uuid: u128,
    pub name: String,
}

/// What an own-linking command answers, and whether the sender is disconnected after it.
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub lines: Vec<String>,
    pub kick: Option<String>,
}

fn say(line: impl Into<String>) -> Reply {
    Reply {
        lines: vec![line.into()],
        kick: None,
    }
}

/// `/linkaccount` with own linking (`LinkAccountCommand`): a Java player names the Bedrock account and gets a
/// code; the Bedrock player names the Java account and enters the code. `bedrock` is the sender's Floodgate id
/// and name when they are a Bedrock player. The wording is the port's own.
pub fn link_own(
    own: &mut Own,
    sender: &Sender,
    bedrock: Option<(u128, String)>,
    target: Option<&str>,
    code: Option<&str>,
    now: Instant,
    timeout: Duration,
) -> Reply {
    match bedrock {
        None => {
            let Some(target) = target else {
                return say("Usage: /linkaccount <bedrock username>");
            };
            match own.store.by_java(sender.uuid) {
                Ok(Some(_)) => {
                    return say("Your account is already linked. Unlink it first with /unlinkaccount.");
                }
                Ok(None) => {}
                Err(e) => return say(format!("Linking failed: {e}")),
            }
            let code = own.request(sender.uuid, &sender.name, target, store::new_code(), now);
            say(format!(
                "Log in on Bedrock as {target} and run /linkaccount {} {code} within {} seconds.",
                sender.name,
                timeout.as_secs()
            ))
        }
        Some((id, name)) => {
            let (Some(target), Some(code)) = (target, code) else {
                return say("Usage: /linkaccount <java username> <code>");
            };
            match own.complete(target, id, &name, code, now, timeout) {
                Ok(l) => Reply {
                    lines: vec![format!("Your account is now linked to {}.", l.java_username)],
                    kick: Some(format!(
                        "Your account is now linked to {}. Join again to play as that account.",
                        l.java_username
                    )),
                },
                Err(LinkError::NoRequest) => say(format!("There is no link request from {target} for {name}.")),
                Err(LinkError::WrongCode) => say("That code is wrong."),
                Err(LinkError::Expired) => say("That link request has expired. Start again on the Java account."),
                Err(LinkError::Store(e)) => say(format!("Linking failed: {e}")),
            }
        }
    }
}

/// `/unlinkaccount` with own linking: a Java player removes the link of their account, a Bedrock player the
/// link of theirs.
pub fn unlink_own(own: &mut Own, sender: &Sender, bedrock: Option<(u128, String)>) -> Reply {
    let removed = match bedrock {
        Some((id, _)) => own.store.unlink_bedrock(id),
        None => own.store.unlink_java(sender.uuid),
    };
    match removed {
        Ok(true) => say("Your account is no longer linked."),
        Ok(false) => say("Your account is not linked."),
        Err(e) => say(format!("Unlinking failed: {e}")),
    }
}

/// The sender as a Bedrock player: their Floodgate id and Bedrock name.
fn bedrock_of(sender: &Sender) -> Option<(u128, String)> {
    crate::player(sender.uuid).map(|p| (p.java_uuid, p.data.username))
}

fn own_reply(sender: &Sender, words: &mut std::str::SplitWhitespace<'_>, link: bool, timeout: i64) -> Reply {
    let (target, code) = (words.next(), words.next());
    let bedrock = bedrock_of(sender);
    let done = store::with(|own| {
        if link {
            link_own(own, sender, bedrock.clone(), target, code, Instant::now(), Duration::from_secs(timeout.max(0) as u64))
        } else {
            unlink_own(own, sender, bedrock.clone())
        }
    });
    let reply = done.unwrap_or_else(|e| say(format!("Linking is unavailable: {e}")));
    if let Some(reason) = &reply.kick {
        crate::channels::kick(sender.uuid, reason);
    }
    reply
}


/// The operator level `PermissionDefault.OP` maps to here.
pub const OP_LEVEL: u8 = 1;

/// The usages to register (`native_register_commands`), with the level each needs and the permission node
/// Floodgate's `Permission` enum gives it. `floodgate` is `floodgate.command.floodgate` (default true, so
/// everyone; the help lists what each may use), `floodgate version` is `floodgate.command.floodgate.version`
/// (operators), `linkaccount` / `unlinkaccount` are `floodgate.command.linkaccount` / `...unlinkaccount`.
pub fn usages(config: &LinkConfig) -> Vec<(&'static str, u8, Option<&'static str>)> {
    let mut u = vec![
        ("floodgate", 0, Some("floodgate.command.floodgate")),
        ("floodgate version", OP_LEVEL, Some("floodgate.command.floodgate.version")),
    ];
    if config.register_commands() {
        // LinkAccountCommand: `linkaccount <player>` with an optional `[code]` (a Bedrock player enters the code).
        u.extend([
            ("linkaccount <player:player_name> [code:word]", 0, Some("floodgate.command.linkaccount")),
            ("unlinkaccount", 0, Some("floodgate.command.unlinkaccount")),
        ]);
    }
    u
}

/// The answer to a command line (without its slash), or `None` when it isn't one of Floodgate's.
pub fn run(line: &str, sender: Option<&Sender>, config: &LinkConfig) -> Option<Vec<String>> {
    let mut words = line.split_whitespace();
    let name = words.next()?.to_ascii_lowercase();
    match name.as_str() {
        "linkaccount" | "unlinkaccount" if config.register_commands() => {
            let Some(sender) = sender else {
                return Some(vec![NOT_A_PLAYER.into()]);
            };
            if config.own() {
                let link = name == "linkaccount";
                return Some(own_reply(sender, &mut words, link, config.link_code_timeout).lines);
            }
            // With only Global Linking (no database implementation) both commands just point there; without
            // any linking they say it is off. Floodgate goes on to its database only when it has one.
            Some(vec![match config.linking() {
                Linking::Global => global_linking_notice(),
                Linking::Disabled => LINKING_DISABLED.into(),
            }])
        }
        "floodgate" => {
            let level = sender.map_or(4, |s| s.level);
            match words.next().map(str::to_ascii_lowercase).as_deref() {
                None => {
                    let mut help = String::from("Available subcommands are:\n");
                    if level >= OP_LEVEL {
                        help.push_str("\n\u{a7}bversion\u{a7}f - \u{a7}7Displays version information about Floodgate");
                    }
                    Some(vec![help])
                }
                Some("version") if level >= OP_LEVEL => Some(vec![format!(
                    "\u{a7}7You're currently on \u{a7}bhorizon-floodgate {}\u{a7}7 (Floodgate data version {})",
                    env!("CARGO_PKG_VERSION"),
                    crate::cipher::VERSION
                )]),
                Some("version") => Some(vec![NO_PERMISSION.into()]),
                Some(_) => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linking_commands_follow_the_config() {
        let player = Sender { level: 0, uuid: 1, name: "Steve".into() };
        let c = LinkConfig::default();
        assert_eq!(
            run("linkaccount Steve", Some(&player), &c),
            Some(vec![global_linking_notice()])
        );
        assert_eq!(
            run("unlinkaccount", Some(&player), &c),
            Some(vec![global_linking_notice()])
        );
        assert_eq!(
            run("linkaccount Steve", None, &c),
            Some(vec![NOT_A_PLAYER.into()])
        );
        let own_only = LinkConfig {
            enable_global_linking: false,
            enable_own_linking: true,
            ..LinkConfig::default()
        };
        // Own linking answers from its database (store.rs tests), not with the Global Linking notice.
        assert!(own_only.register_commands() && own_only.own());
        let off = LinkConfig {
            enabled: false,
            ..LinkConfig::default()
        };
        assert_eq!(run("linkaccount x", Some(&player), &off), None);
        assert!(!usages(&off)
            .iter()
            .any(|(u, _, _)| u.starts_with("linkaccount")));
        assert_eq!(usages(&c).len(), 4);
        assert!(usages(&c)
            .iter()
            .any(|(u, _, _)| *u == "linkaccount <player:player_name> [code:word]"));
    }

    #[test]
    fn main_command() {
        let c = LinkConfig::default();
        let player = Sender { level: 0, uuid: 1, name: "Steve".into() };
        let op = Sender { level: 4, uuid: 2, name: "Op".into() };
        assert_eq!(
            run("floodgate", Some(&player), &c),
            Some(vec!["Available subcommands are:\n".into()])
        );
        assert!(run("floodgate", Some(&op), &c).unwrap()[0].contains("version"));
        assert!(run("floodgate version", Some(&op), &c).unwrap()[0].contains("horizon-floodgate"));
        assert_eq!(
            run("floodgate version", Some(&player), &c),
            Some(vec![NO_PERMISSION.into()])
        );
        assert_eq!(run("floodgate firewall", Some(&op), &c), None);
        assert_eq!(run("gamemode 1", Some(&op), &c), None);
    }

    #[test]
    fn own_linking_flow() {
        let mut own = Own::new(store::Store::open_memory().unwrap());
        let java = Sender { level: 0, uuid: 0xAA, name: "Alex".into() };
        let bedrock = Sender { level: 0, uuid: 9, name: ".Steve".into() };
        let now = Instant::now();
        let ten = Duration::from_secs(300);
        let r = link_own(&mut own, &java, None, None, None, now, ten);
        assert!(r.lines[0].starts_with("Usage"));
        let r = link_own(&mut own, &java, None, Some("Steve"), None, now, ten);
        let code = r.lines[0].split_whitespace().rev().nth(3).unwrap().to_string();
        assert!(r.lines[0].contains("/linkaccount Alex "), "{r:?}");
        let b = || Some((9u128, "Steve".to_string()));
        let wrong = link_own(&mut own, &bedrock, b(), Some("Alex"), Some("x"), now, ten);
        assert_eq!(wrong.lines, vec!["That code is wrong.".to_string()]);
        assert_eq!(wrong.kick, None);
        let done = link_own(&mut own, &bedrock, b(), Some("Alex"), Some(&code), now, ten);
        assert!(done.kick.is_some(), "{done:?}");
        assert_eq!(own.store.by_bedrock(9).unwrap().unwrap().java_uuid, 0xAA);
        // Linked: the Java side cannot start another, the Bedrock side can unlink.
        let again = link_own(&mut own, &java, None, Some("Steve"), None, now, ten);
        assert!(again.lines[0].contains("already linked"));
        assert_eq!(unlink_own(&mut own, &bedrock, b()).lines, vec!["Your account is no longer linked.".to_string()]);
        assert_eq!(unlink_own(&mut own, &java, None).lines, vec!["Your account is not linked.".to_string()]);
    }
}
