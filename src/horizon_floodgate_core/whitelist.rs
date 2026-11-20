//! `/fwhitelist` (GeyserMC's `WhitelistCommand`, MIT, see LICENSE): whitelists a Bedrock player by their
//! Floodgate UUID (`00000000-0000-0000-<xuid>`) so they can join before they ever played. Horizon's mod API has
//! no whitelist call, so the entry is written to `whitelist.json` (the server's working directory, vanilla's
//! format) and the server is told to read the file again (`whitelist reload`, run by the mod's tick through
//! [`take_reload`]). The wording of the answers is the port's own (Floodgate's translations have no licence).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::horizon_floodgate_core::data::uuid_string;

/// The file the server's whitelist lives in (vanilla's name, relative to the working directory).
pub const FILE: &str = "whitelist.json";

static RELOAD: AtomicBool = AtomicBool::new(false);

/// Whether the whitelist file changed since the last call: the mod then runs `whitelist reload`.
pub fn take_reload() -> bool {
    RELOAD.swap(false, Ordering::Relaxed)
}

/// `Message.INVALID_USERNAME`.
pub const INVALID_USERNAME: &str = "Invalid username or UUID given.";
/// `Message.API_UNAVAILABLE` (with `CHECK_CONSOLE`).
pub const API_UNAVAILABLE: &str = "Couldn't reach the Global API. Check the console for details.";
/// `Message.USER_NOT_FOUND`.
pub const USER_NOT_FOUND: &str = "Couldn't find a Bedrock player with that name.";
/// `Message.UNEXPECTED_ERROR` (with `CHECK_CONSOLE`).
pub const UNEXPECTED_ERROR: &str = "An unexpected error happened. Check the console for details.";

/// `Message.PLAYER_ADDED` and its three siblings, for `who` (a name, or a UUID).
pub fn result_line(add: bool, changed: bool, who: &str) -> String {
    match (add, changed) {
        (true, true) => format!("Added {who} to the whitelist."),
        (true, false) => format!("{who} is already whitelisted."),
        (false, true) => format!("Removed {who} from the whitelist."),
        (false, false) => format!("{who} is not whitelisted."),
    }
}

/// What the typed player argument is.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A UUID (typed, or the one of an online Bedrock player).
    Uuid(u128),
    /// A Bedrock username (without the prefix still to strip), to be looked up in the Global API.
    Name(String),
}

/// `PlayerAudienceArgument.ofAnyIdentifierBedrock`: more than 16 characters must be a UUID (32 or 36 characters);
/// a shorter text is a name. `online` finds an online Bedrock player by their name (then it is their UUID).
pub fn parse_target(input: &str, online: &dyn Fn(&str) -> Option<u128>) -> Result<Target, String> {
    let input = input.trim_matches('"');
    if input.chars().count() > 16 {
        if !input.is_ascii() {
            return Err("Expected player name/UUID".into());
        }
        let dashed = match input.len() {
            36 => input.to_string(),
            32 => format!("{}-{}-{}-{}-{}", &input[0..8], &input[8..12], &input[12..16], &input[16..20], &input[20..32]),
            _ => return Err("Expected player name/UUID".into()),
        };
        return crate::horizon_floodgate_core::data::parse_uuid(&dashed)
            .map(Target::Uuid)
            .map_err(|_| format!("Invalid UUID '{input}'"));
    }
    Ok(match online(input) {
        Some(uuid) => Target::Uuid(uuid),
        None => Target::Name(input.to_string()),
    })
}

/// The name a lookup uses and the one the entry gets: `(stripped, correct)` after Floodgate's prefix handling
/// (`WhitelistCommand.performCommand`), or None when the name is empty or longer than 16 characters.
pub fn names(name: &str, prefix: &str, replace_spaces: bool) -> Option<(String, String)> {
    let stripped = name.strip_prefix(prefix).unwrap_or(name);
    if stripped.is_empty() || stripped.chars().count() > 16 {
        return None;
    }
    let temp = if replace_spaces { stripped.replace(' ', "_") } else { stripped.to_string() };
    Some((stripped.to_string(), format!("{prefix}{temp}")))
}

/// Adds or removes `uuid` in the whitelist file at `path`; true when the file changed (false: it was
/// already so). The file is vanilla's: a JSON array of `{"uuid": "<dashed>", "name": "<name>"}`.
pub fn apply(path: &Path, uuid: u128, name: &str, add: bool) -> Result<bool, String> {
    let mut entries: Vec<serde_json::Value> = match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Vec::new(),
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let wanted = uuid_string(uuid);
    let at = entries
        .iter()
        .position(|e| e.get("uuid").and_then(|u| u.as_str()).is_some_and(|u| u.eq_ignore_ascii_case(&wanted)));
    match (add, at) {
        (true, Some(_)) | (false, None) => return Ok(false),
        (true, None) => entries.push(serde_json::json!({ "uuid": wanted, "name": name })),
        (false, Some(i)) => {
            entries.remove(i);
        }
    }
    let text = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)).map_err(|e| format!("{}: {e}", path.display()))?;
    RELOAD.store(true, Ordering::Relaxed);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_follow_the_argument_rules() {
        let none = |_: &str| None;
        assert_eq!(parse_target(".Steve", &none), Ok(Target::Name(".Steve".into())));
        assert_eq!(parse_target("\"Alex\"", &none), Ok(Target::Name("Alex".into())));
        assert_eq!(parse_target("Steve", &|n| (n == "Steve").then_some(9)), Ok(Target::Uuid(9)));
        let id = "00000000-0000-0000-0009-000000000abc";
        assert_eq!(parse_target(id, &none), Ok(Target::Uuid(0x0009_0000_0000_0abc)));
        assert_eq!(parse_target(&id.replace('-', ""), &none), Ok(Target::Uuid(0x0009_0000_0000_0abc)));
        assert_eq!(parse_target("a-name-that-is-way-too-long", &none), Err("Expected player name/UUID".into()));
    }

    #[test]
    fn names_strip_the_prefix_and_replace_spaces() {
        assert_eq!(names(".Steve Jobs", ".", true), Some(("Steve Jobs".into(), ".Steve_Jobs".into())));
        assert_eq!(names("Steve Jobs", ".", false), Some(("Steve Jobs".into(), ".Steve Jobs".into())));
        assert_eq!(names(".", ".", true), None);
        assert_eq!(names("12345678901234567", ".", true), None);
    }

    #[test]
    fn the_file_gets_and_loses_entries() {
        let dir = std::env::temp_dir().join(format!("fg-wl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("whitelist.json");
        let _ = std::fs::remove_file(&path);
        take_reload();
        assert_eq!(apply(&path, 0xabc, ".Steve", true), Ok(true));
        assert!(take_reload());
        assert_eq!(apply(&path, 0xabc, ".Steve", true), Ok(false));
        assert!(!take_reload());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"uuid\": \"00000000-0000-0000-0000-000000000abc\""), "{text}");
        assert!(text.contains("\"name\": \".Steve\""), "{text}");
        assert_eq!(apply(&path, 0xdef, "x", false), Ok(false));
        assert_eq!(apply(&path, 0xabc, "x", false), Ok(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "[]");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_result_lines_say_what_happened() {
        assert_eq!(result_line(true, true, "Steve"), "Added Steve to the whitelist.");
        assert_eq!(result_line(false, false, "Steve"), "Steve is not whitelisted.");
    }
}
