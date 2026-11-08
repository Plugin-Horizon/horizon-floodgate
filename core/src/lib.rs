//! Floodgate's backend part for Horizon: a Floodgate proxy (Velocity with `send-floodgate-data: true`)
//! puts the Bedrock player's data, encrypted with the shared key, into the handshake's host after a NUL.
//! This crate finds it, decrypts it, refuses the login with Floodgate's messages when it is wrong, and keeps
//! the Bedrock player's data while they are online. The player's name and UUID themselves come from the
//! proxy's Velocity forwarding, as on a Paper backend with proxy data.
//!
//! The flow follows GeyserMC's Floodgate (MIT, see LICENSE): `FloodgateHandshakeHandler`,
//! `CommonDataHandler`, `HandshakeDataImpl`.

pub mod channels;
pub mod cipher;
pub mod commands;
pub mod data;
pub mod forms;
pub mod link;
pub mod store;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use cipher::{DecryptError, Key};
use data::{BedrockData, ParseError};

/// Where the config and the key live, relative to the server's folder.
pub const CONFIG_DIR: &str = "config/floodgate";
/// The config written when there is none.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");

/// `Constants.INTERNAL_ERROR_MESSAGE`.
pub const INTERNAL_ERROR_MESSAGE: &str =
    "An internal error happened while handling Floodgate data. \
Try logging in again or contact a server administrator if the issue persists.";

/// `Constants.UNSUPPORTED_DATA_VERSION`, formatted.
pub fn unsupported_data_version(received: i32) -> String {
    format!(
        "Received an unsupported Floodgate data version. This Floodgate version is made for data \
version {}, received {received}. Make sure that Floodgate is up-to-date.",
        cipher::VERSION
    )
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Config {
    pub key_file_name: String,
    pub username_prefix: String,
    pub replace_spaces: bool,
    pub disconnect: Disconnect,
    pub player_link: link::LinkConfig,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Disconnect {
    pub invalid_key: String,
    pub invalid_arguments_length: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            key_file_name: "key.pem".into(),
            username_prefix: ".".into(),
            replace_spaces: true,
            disconnect: Disconnect::default(),
            player_link: link::LinkConfig::default(),
        }
    }
}

impl Default for Disconnect {
    fn default() -> Self {
        Self {
            invalid_key: "Please connect through the official Geyser".into(),
            invalid_arguments_length: "Expected {} arguments, got {}. Is Geyser up-to-date?".into(),
        }
    }
}

/// A Bedrock player as Floodgate's `FloodgatePlayer` knows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub data: BedrockData,
    /// The prefixed Bedrock name and `00000000-0000-0000-<xuid>`.
    pub java_username: String,
    pub java_uuid: u128,
}

impl Player {
    /// `getCorrectUsername`: the linked Java account's name, else the prefixed one.
    pub fn correct_username(&self) -> &str {
        match &self.data.linked_player {
            Some(l) => &l.java_username,
            None => &self.java_username,
        }
    }
    /// `getCorrectUniqueId`.
    pub fn correct_uuid(&self) -> u128 {
        match &self.data.linked_player {
            Some(l) => l.java_uuid,
            None => self.java_uuid,
        }
    }
    /// The line Floodgate logs when the player joins (`floodgate.ingame.login_name`, en_US).
    pub fn login_line(&self) -> String {
        format!(
            "Floodgate player logged in as {} joined (UUID: {})",
            self.correct_username(),
            data::uuid_string(self.correct_uuid())
        )
    }
}

/// What a handshake's host says.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Valid Floodgate data.
    Bedrock(Box<Player>),
    /// Floodgate data that must be refused, with the message the player gets.
    Kick(String),
}

/// `FloodgateHandshakeHandler.separateHostname`: the first NUL-separated part that is Floodgate data, its
/// version, and the host without it.
#[derive(Debug, PartialEq, Eq)]
pub struct Separation {
    pub floodgate_data: Option<String>,
    pub version: i32,
    pub hostname: String,
}

pub fn separate_hostname(host: &str) -> Separation {
    let mut floodgate_data = None;
    let mut version = -1;
    let mut rest = Vec::new();
    for part in data::java_split(host, '\0') {
        match cipher::version(part) {
            Some(v) if floodgate_data.is_none() => {
                floodgate_data = Some(part.to_string());
                version = v;
            }
            _ => rest.push(part),
        }
    }
    Separation {
        floodgate_data,
        version,
        hostname: rest.join("\0"),
    }
}

/// The checks of `CommonDataHandler.handle` and `FloodgateHandshakeHandler.handle` on a backend that gets
/// proxy data. `None`: no Floodgate data, a normal Java login.
pub fn handle_host(host: &str, config: &Config, key: Option<&Key>) -> Option<Outcome> {
    let separation = separate_hostname(host);
    let floodgate = separation.floodgate_data?;
    if separation.version != cipher::VERSION {
        return Some(Outcome::Kick(unsupported_data_version(separation.version)));
    }
    let invalid_key = || Some(Outcome::Kick(config.disconnect.invalid_key.clone()));
    let Some(key) = key else {
        return invalid_key();
    };
    let plain = match key.decrypt(floodgate.as_bytes()) {
        Ok(plain) => plain,
        Err(DecryptError::InvalidFormat) => return None,
        Err(DecryptError::Failed) => return invalid_key(),
    };
    let internal = || Some(Outcome::Kick(INTERNAL_ERROR_MESSAGE.to_string()));
    let data = match BedrockData::parse(&String::from_utf8_lossy(&plain)) {
        Ok(data) => data,
        Err(ParseError::Length(_)) => {
            return Some(Outcome::Kick(
                config.disconnect.invalid_arguments_length.clone(),
            ))
        }
        Err(ParseError::Field(_)) => return internal(),
    };
    let Ok(java_uuid) = data.java_uuid() else {
        return internal();
    };
    let java_username = data.java_username(&config.username_prefix, config.replace_spaces);
    Some(Outcome::Bedrock(Box::new(Player {
        data,
        java_username,
        java_uuid,
    })))
}

/// The link of a Bedrock player: the own database first when own linking is on, then Global Linking
/// (`LocalPlayerLinking` and `GlobalPlayerLinking` together).
pub fn find_link(config: &link::LinkConfig, xuid: i64) -> Result<Option<data::LinkedPlayer>, String> {
    if config.own() {
        match store::with(|o| o.store.by_bedrock(u128::from(xuid as u64)))? {
            Ok(Some(found)) => return Ok(Some(found)),
            Ok(None) => {}
            Err(e) => return Err(format!("Failed to read the link database: {e}")),
        }
    }
    if config.linking() == link::Linking::Global {
        return link::fetch_global_link(xuid);
    }
    Ok(None)
}

/// `fetchLinkedPlayer` and `handlePart2`'s `require-link` check, after `handle_host` accepted the data: a Bedrock
/// player the proxy sent without a link is looked up with `lookup(xuid)` (Global Linking), and one still without a
/// link is refused when `require-link` is on. As on a Paper backend with proxy data, a link found here only goes
/// into the player's Floodgate data: the name and UUID they play with stay the proxy's.
pub fn resolve_link(
    outcome: Option<Outcome>,
    config: &link::LinkConfig,
    lookup: &dyn Fn(i64) -> Result<Option<data::LinkedPlayer>, String>,
    log: &dyn Fn(&str, &str),
) -> Option<Outcome> {
    let Some(Outcome::Bedrock(mut player)) = outcome else {
        return outcome;
    };
    if player.data.linked_player.is_none()
        && (config.linking() == link::Linking::Global || config.own())
    {
        if let Ok(xuid) = player.data.xuid.parse::<i64>() {
            match lookup(xuid) {
                Ok(found) => player.data.linked_player = found,
                Err(line) => log("ERROR", &format!("[floodgate] {line}")),
            }
        }
    }
    if config.require_link && player.data.linked_player.is_none() {
        return Some(Outcome::Kick(link::not_linked_message()));
    }
    Some(Outcome::Bedrock(player))
}

struct State {
    config: Config,
    key: Option<Key>,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Reads `config/floodgate/config.toml` (written from the default when missing) and the key file once.
/// A missing or unreadable key refuses every Floodgate login with `invalid-key`, which is what a Floodgate
/// backend with a key other than the proxy's does. `log(level, line)` gets the problems.
fn state(log: &dyn Fn(&str, &str)) -> &'static State {
    STATE.get_or_init(|| {
        let dir = std::path::Path::new(CONFIG_DIR);
        let path = dir.join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(&path, DEFAULT_CONFIG);
                DEFAULT_CONFIG.to_string()
            }
        };
        let config = toml::from_str::<Config>(&text).unwrap_or_else(|e| {
            log("ERROR", &format!("[floodgate] {}: {e}; using the defaults", path.display()));
            Config::default()
        });
        let key_path = dir.join(&config.key_file_name);
        let key = match std::fs::read(&key_path).map_err(|e| e.to_string()).and_then(|b| Key::new(&b)) {
            Ok(key) => Some(key),
            Err(e) => {
                log(
                    "ERROR",
                    &format!(
                        "[floodgate] no usable key at {} ({e}): every Bedrock login is refused. Copy the \
proxy's key there and restart.",
                        key_path.display()
                    ),
                );
                None
            }
        };
        State { config, key }
    })
}

/// What a handshake frame at the start of a connection holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Peek {
    /// Not all of the frame has arrived.
    NeedMore,
    /// Not a handshake this crate reads (a legacy ping, a malformed frame): the engine decides.
    Other,
    Handshake {
        host: String,
        intent: i32,
    },
}

/// Largest handshake frame looked at: a 32767-character host in UTF-8 and the other fields.
const MAX_FRAME: usize = 3 * 32767 + 32;

fn varint(buf: &[u8], pos: &mut usize) -> Option<Result<i32, ()>> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let Some(&b) = buf.get(*pos) else {
            return None;
        };
        *pos += 1;
        value |= u32::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            return Some(Ok(value as i32));
        }
    }
    Some(Err(()))
}

/// Reads the handshake (`ClientIntentionPacket`, id 0, uncompressed) at the start of `buf`.
pub fn peek_handshake(buf: &[u8]) -> Peek {
    if buf.first() == Some(&0xFE) {
        return Peek::Other;
    }
    let mut pos = 0;
    let length = match varint(buf, &mut pos) {
        None => return Peek::NeedMore,
        Some(Ok(n)) if n > 0 && (n as usize) <= MAX_FRAME => n as usize,
        Some(_) => return Peek::Other,
    };
    let Some(frame) = buf.get(pos..pos + length) else {
        return Peek::NeedMore;
    };
    let mut at = 0;
    let mut next = || match varint(frame, &mut at) {
        Some(Ok(v)) => Some(v),
        _ => None,
    };
    let (Some(0), Some(_protocol), Some(len)) = (next(), next(), next()) else {
        return Peek::Other;
    };
    let Some(host) = usize::try_from(len)
        .ok()
        .and_then(|len| frame.get(at..at + len))
    else {
        return Peek::Other;
    };
    let Ok(host) = std::str::from_utf8(host) else {
        return Peek::Other;
    };
    at += host.len() + 2; // the port
    match varint(frame, &mut at) {
        Some(Ok(intent)) => Peek::Handshake {
            host: host.to_string(),
            intent,
        },
        _ => Peek::Other,
    }
}

thread_local! {
    /// The outcome of this connection thread's handshake, until its login takes it.
    static PENDING: RefCell<Option<Outcome>> = const { RefCell::new(None) };
}

/// At the start of a connection, before the engine reads it: looks at (without consuming) the handshake
/// and keeps what its Floodgate data says for this thread's login. Logins only (intents 2 and 3).
pub fn on_connection(stream: &std::net::TcpStream, log: &dyn Fn(&str, &str)) {
    PENDING.with(|p| *p.borrow_mut() = None);
    let previous = stream.read_timeout().ok().flatten();
    let wait = Duration::from_secs(10);
    let _ = stream.set_read_timeout(Some(wait));
    let deadline = Instant::now() + wait;
    let mut buf = vec![0u8; MAX_FRAME + 8];
    let outcome = loop {
        match stream.peek(&mut buf) {
            Ok(n) if n > 0 => match peek_handshake(&buf[..n]) {
                Peek::NeedMore if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Peek::Handshake { host, intent } if intent == 2 || intent == 3 => {
                    let state = state(log);
                    let outcome = handle_host(&host, &state.config, state.key.as_ref());
                    break resolve_link(
                        outcome,
                        &state.config.player_link,
                        &|xuid| find_link(&state.config.player_link, xuid),
                        log,
                    );
                }
                _ => break None,
            },
            _ => break None,
        }
    };
    let _ = stream.set_read_timeout(previous);
    PENDING.with(|p| *p.borrow_mut() = outcome);
}

/// `native_handshake` (hooks-all), on the connection's thread: for a login or a transfer, decrypts the host's
/// Floodgate data, puts the plain hostname back into `host` (as Floodgate does) and keeps the outcome for this
/// thread's login. Some is a refusal for the handshake to disconnect with. Nothing blocks here: Global Linking
/// waits for [`on_pre_login`].
pub fn on_handshake(host: &mut String, intent: i32, log: &dyn Fn(&str, &str)) -> Option<String> {
    PENDING.with(|p| *p.borrow_mut() = None);
    if intent != 2 && intent != 3 {
        return None;
    }
    let state = state(log);
    let outcome = handle_host(host.as_str(), &state.config, state.key.as_ref())?;
    *host = separate_hostname(host.as_str()).hostname;
    match outcome {
        Outcome::Kick(text) => Some(text),
        bedrock => {
            PENDING.with(|p| *p.borrow_mut() = Some(bedrock));
            None
        }
    }
}

/// `native_player_pre_login` (hooks-all), on the same connection thread: the Global Linking lookup and
/// `require-link` for this thread's Bedrock login (blocking, bounded by the lookup's own timeout). Err refuses
/// the login with Floodgate's message; a Bedrock player stays pending for [`on_profile`].
pub fn on_pre_login(log: &dyn Fn(&str, &str)) -> Result<(), String> {
    let config = &state(log).config;
    pre_login_with(config, &|xuid| find_link(&config.player_link, xuid), log)
}

fn pre_login_with(
    config: &Config,
    lookup: &dyn Fn(i64) -> Result<Option<data::LinkedPlayer>, String>,
    log: &dyn Fn(&str, &str),
) -> Result<(), String> {
    let pending = take_pending();
    if pending.is_none() {
        return Ok(());
    }
    match resolve_link(pending, &config.player_link, lookup, log) {
        Some(Outcome::Kick(text)) => Err(text),
        other => {
            PENDING.with(|p| *p.borrow_mut() = other);
            Ok(())
        }
    }
}

/// `native_player_profile` (hooks-all), on the same connection thread once the server let the player in: a
/// pending Bedrock login becomes a Floodgate player. Gives Floodgate's login line when the player is who the
/// data says.
pub fn on_profile(player: u128) -> Option<String> {
    let Some(Outcome::Bedrock(bedrock)) = take_pending() else {
        return None;
    };
    let line = (bedrock.correct_uuid() == player).then(|| bedrock.login_line());
    add_player(*bedrock);
    line
}

/// This thread's pending outcome, taken (the login asks once).
pub fn take_pending() -> Option<Outcome> {
    PENDING.with(|p| p.borrow_mut().take())
}

static PLAYERS: OnceLock<Mutex<HashMap<u128, Player>>> = OnceLock::new();

fn players() -> std::sync::MutexGuard<'static, HashMap<u128, Player>> {
    PLAYERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// `SimpleFloodgateApi.addPlayer`: kept under the Floodgate UUID (`00000000-0000-0000-<xuid>`).
pub fn add_player(player: Player) {
    players().insert(player.java_uuid, player);
}

/// The key of the player with this UUID: their Floodgate UUID, or the UUID of their linked Java account.
fn key_of(map: &HashMap<u128, Player>, uuid: u128) -> Option<u128> {
    if map.contains_key(&uuid) {
        return Some(uuid);
    }
    map.values()
        .find(|p| p.correct_uuid() == uuid)
        .map(|p| p.java_uuid)
}

/// `SimpleFloodgateApi.playerRemoved`, by either UUID. Their open forms are answered as closed.
pub fn remove_player(uuid: u128) -> Option<Player> {
    let removed = {
        let mut map = players();
        key_of(&map, uuid).and_then(|k| map.remove(&k))
    };
    channels::player_left(uuid);
    if let Some(p) = &removed {
        for other in [p.java_uuid, p.correct_uuid()] {
            if other != uuid {
                channels::player_left(other);
            }
        }
    }
    removed
}

/// `FloodgateApi.getPlayer`: by the Floodgate UUID or a linked player's Java UUID.
pub fn player(uuid: u128) -> Option<Player> {
    let map = players();
    key_of(&map, uuid).and_then(|k| map.get(&k).cloned())
}

/// `FloodgateApi.getPlayers`.
pub fn all_players() -> Vec<Player> {
    players().values().cloned().collect()
}

/// `FloodgateApi.getPlayerCount`.
pub fn player_count() -> usize {
    players().len()
}

/// `FloodgateApi.isFloodgateId`: the UUID's upper half is zero.
pub fn is_floodgate_id(uuid: u128) -> bool {
    uuid >> 64 == 0
}

/// `FloodgateApi.createJavaPlayerId`.
pub fn create_java_player_id(xuid: i64) -> u128 {
    u128::from(xuid as u64)
}

/// The server's Floodgate config (read on first use).
pub fn config() -> Config {
    state(&|level, line| eprintln!("[{level}] {line}"))
        .config
        .clone()
}

/// `FloodgateApi.isFloodgatePlayer`.
pub fn is_floodgate_player(uuid: u128) -> bool {
    players().contains_key(&uuid)
}

/// A plain text component as JSON, for a login disconnect.
pub fn json_text(text: &str) -> String {
    let mut out = String::from("{\"text\":\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push_str("\"}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        Key::new(&[3u8; 16]).unwrap()
    }

    fn data() -> BedrockData {
        BedrockData::parse(
            "1.21.130\0Steve Two\02535400000000001\07\0en_US\00\01\0203.0.113.9\0null\01\042\0abc",
        )
        .unwrap()
    }

    fn host_with(payload: &[u8]) -> String {
        format!(
            "play.example.org\0{}",
            String::from_utf8(payload.to_vec()).unwrap()
        )
    }

    #[test]
    fn separates_like_floodgate() {
        let s = separate_hostname("a.example\0^Floodgate^>xyz\0rest");
        assert_eq!(s.floodgate_data.as_deref(), Some("^Floodgate^>xyz"));
        assert_eq!(s.version, 0);
        assert_eq!(s.hostname, "a.example\0rest");
        let none = separate_hostname("a.example");
        assert_eq!(none.floodgate_data, None);
        assert_eq!(none.hostname, "a.example");
    }

    #[test]
    fn valid_data_is_a_bedrock_player() {
        let k = key();
        let host = host_with(&k.encrypt(data().to_wire().as_bytes()).unwrap());
        let Some(Outcome::Bedrock(p)) = handle_host(&host, &Config::default(), Some(&k)) else {
            panic!("not accepted");
        };
        assert_eq!(p.correct_username(), ".Steve_Two");
        assert_eq!(
            p.login_line(),
            "Floodgate player logged in as .Steve_Two joined (UUID: 00000000-0000-0000-0009-01eed05d1001)"
        );
    }

    #[test]
    fn refusals() {
        let k = key();
        let config = Config::default();
        let other = Key::new(&[4u8; 16]).unwrap();
        let host = host_with(&other.encrypt(data().to_wire().as_bytes()).unwrap());
        assert_eq!(
            handle_host(&host, &config, Some(&k)),
            Some(Outcome::Kick(config.disconnect.invalid_key.clone()))
        );
        assert_eq!(
            handle_host(&host, &config, None),
            Some(Outcome::Kick(config.disconnect.invalid_key.clone()))
        );
        let short = host_with(&k.encrypt(b"a\0b\0c").unwrap());
        assert_eq!(
            handle_host(&short, &config, Some(&k)),
            Some(Outcome::Kick(
                config.disconnect.invalid_arguments_length.clone()
            ))
        );
        let bad_xuid = data().to_wire().replace("2535400000000001", "x");
        let host = host_with(&k.encrypt(bad_xuid.as_bytes()).unwrap());
        assert_eq!(
            handle_host(&host, &config, Some(&k)),
            Some(Outcome::Kick(INTERNAL_ERROR_MESSAGE.to_string()))
        );
        assert_eq!(
            handle_host("h\0^Floodgate^?abc", &config, Some(&k)),
            Some(Outcome::Kick(unsupported_data_version(1)))
        );
        assert_eq!(handle_host("play.example.org", &config, Some(&k)), None);
    }

    #[test]
    fn config_keys_like_paper() {
        let c: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(c.key_file_name, "key.pem");
        assert_eq!(c.username_prefix, ".");
        assert!(c.replace_spaces);
        assert_eq!(
            c.disconnect.invalid_key,
            "Please connect through the official Geyser"
        );
        let custom: Config = toml::from_str("username-prefix = \"*\"").unwrap();
        assert_eq!(custom.username_prefix, "*");
        assert_eq!(custom.key_file_name, "key.pem");
    }

    fn frame(host: &str, intent: u8) -> Vec<u8> {
        let mut body = vec![0x00, 0x80, 0x06]; // id 0, protocol 768 (any)
        let mut len = host.len();
        loop {
            let b = (len & 0x7F) as u8;
            len >>= 7;
            body.push(if len > 0 { b | 0x80 } else { b });
            if len == 0 {
                break;
            }
        }
        body.extend_from_slice(host.as_bytes());
        body.extend_from_slice(&25565u16.to_be_bytes());
        body.push(intent);
        let mut out = Vec::new();
        let mut len = body.len();
        loop {
            let b = (len & 0x7F) as u8;
            len >>= 7;
            out.push(if len > 0 { b | 0x80 } else { b });
            if len == 0 {
                break;
            }
        }
        out.extend(body);
        out
    }

    #[test]
    fn peeks_the_handshake() {
        let host = "x".repeat(400);
        let f = frame(&host, 2);
        assert_eq!(
            peek_handshake(&f),
            Peek::Handshake {
                host: host.clone(),
                intent: 2
            }
        );
        assert_eq!(peek_handshake(&f[..f.len() - 1]), Peek::NeedMore);
        assert_eq!(peek_handshake(&[]), Peek::NeedMore);
        assert_eq!(peek_handshake(&[0xFE, 0x01]), Peek::Other);
        let mut not_handshake = f.clone();
        not_handshake[2] = 0x01; // packet id 1
        assert_eq!(peek_handshake(&not_handshake), Peek::Other);
    }

    #[test]
    fn link_lookup_and_require_link() {
        let k = key();
        let host = host_with(&k.encrypt(data().to_wire().as_bytes()).unwrap());
        let config = Config::default();
        let quiet = |_: &str, _: &str| {};
        let friend = data::LinkedPlayer {
            java_username: "JavaFriend".into(),
            java_uuid: 0x1234,
            bedrock_id: 9,
        };
        let found = |_: i64| Ok(Some(friend.clone()));
        let none = |_: i64| Ok(None);
        let down = |_: i64| Err("Failed to request link for 1: down".to_string());
        let outcome = || handle_host(&host, &config, Some(&k));
        let Some(Outcome::Bedrock(p)) =
            resolve_link(outcome(), &config.player_link, &found, &quiet)
        else {
            panic!()
        };
        assert_eq!(p.correct_username(), "JavaFriend");
        assert_eq!(p.correct_uuid(), 0x1234);
        let Some(Outcome::Bedrock(p)) = resolve_link(outcome(), &config.player_link, &down, &quiet)
        else {
            panic!()
        };
        assert!(p.data.linked_player.is_none());
        let strict = link::LinkConfig {
            require_link: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_link(outcome(), &strict, &none, &quiet),
            Some(Outcome::Kick(link::not_linked_message()))
        );
        assert!(matches!(
            resolve_link(outcome(), &strict, &found, &quiet),
            Some(Outcome::Bedrock(_))
        ));
        let off = link::LinkConfig {
            enabled: false,
            ..Default::default()
        };
        let never = |_: i64| -> Result<Option<data::LinkedPlayer>, String> { panic!("looked up") };
        assert!(matches!(
            resolve_link(outcome(), &off, &never, &quiet),
            Some(Outcome::Bedrock(_))
        ));
        assert_eq!(resolve_link(None, &strict, &never, &quiet), None);
    }

    #[test]
    fn registry_and_json() {
        let p = Player {
            data: data(),
            java_username: ".Steve_Two".into(),
            java_uuid: 9,
        };
        add_player(p.clone());
        assert!(is_floodgate_player(9));
        assert_eq!(player(9), Some(p.clone()));
        assert!(remove_player(9).is_some());
        assert!(!is_floodgate_player(9));
        let mut linked = p.clone();
        linked.java_uuid = 10;
        linked.data.linked_player = Some(data::LinkedPlayer {
            java_username: "J".into(),
            java_uuid: 0x5555 << 64,
            bedrock_id: 10,
        });
        add_player(linked);
        assert_eq!(player(0x5555 << 64).map(|p| p.java_uuid), Some(10));
        assert!(remove_player(0x5555 << 64).is_some());
        assert!(player(10).is_none());
        assert!(is_floodgate_id(10) && !is_floodgate_id(0x5555 << 64));
        assert_eq!(json_text("a\"b\\c\n"), "{\"text\":\"a\\\"b\\\\c\\u000a\"}");
    }

    #[test]
    fn handshake_pre_login_profile_hand_over_on_one_thread() {
        let k = key();
        let host = host_with(&k.encrypt(data().to_wire().as_bytes()).unwrap());
        // The host the handshake leaves behind is the plain one.
        assert_eq!(separate_hostname(&host).hostname, "play.example.org");
        let quiet = |_: &str, _: &str| {};
        let none = |_: i64| Ok(None);
        // Without a pending Bedrock login, pre-login lets anyone through and looks nothing up.
        PENDING.with(|p| *p.borrow_mut() = None);
        let never = |_: i64| -> Result<Option<data::LinkedPlayer>, String> { panic!("looked up") };
        assert_eq!(pre_login_with(&Config::default(), &never, &quiet), Ok(()));
        assert_eq!(on_profile(1), None);
        // require-link refuses an unlinked Bedrock player at pre-login.
        let strict = Config {
            player_link: link::LinkConfig {
                require_link: true,
                ..Default::default()
            },
            ..Config::default()
        };
        PENDING.with(|p| *p.borrow_mut() = handle_host(&host, &strict, Some(&k)));
        assert_eq!(
            pre_login_with(&strict, &none, &quiet),
            Err(link::not_linked_message())
        );
        assert_eq!(take_pending(), None);
        // An accepted one stays pending and becomes a Floodgate player at the profile.
        let config = Config::default();
        let outcome = handle_host(&host, &config, Some(&k));
        let Some(Outcome::Bedrock(expected)) = &outcome else {
            panic!()
        };
        let uuid = expected.correct_uuid();
        PENDING.with(|p| *p.borrow_mut() = outcome.clone_bedrock());
        assert_eq!(pre_login_with(&config, &none, &quiet), Ok(()));
        assert!(on_profile(uuid).is_some_and(|l| l.contains("joined")));
        assert!(is_floodgate_player(uuid));
        assert_eq!(on_profile(uuid), None, "taken once");
        remove_player(uuid);
    }

    trait CloneBedrock {
        fn clone_bedrock(&self) -> Option<Outcome>;
    }
    impl CloneBedrock for Option<Outcome> {
        fn clone_bedrock(&self) -> Option<Outcome> {
            match self {
                Some(Outcome::Bedrock(p)) => Some(Outcome::Bedrock(p.clone())),
                Some(Outcome::Kick(t)) => Some(Outcome::Kick(t.clone())),
                None => None,
            }
        }
    }
}
