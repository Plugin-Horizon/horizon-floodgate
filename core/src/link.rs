//! Player linking on the backend (GeyserMC's `PlayerLinkHolder`, `GlobalPlayerLinking`, `HttpClient` and the
//! handshake's `fetchLinkedPlayer`, MIT, see LICENSE), and the Global API's other lookups
//! (`SimpleFloodgateApi.getXuidFor`/`getGamertagFor`).
//!
//! Global Linking is here; own linking (`enable-own-linking`, SQLite) is in `store.rs` and the commands.

use std::time::Duration;

use crate::data::{parse_uuid, LinkedPlayer};

/// `Constants.GET_BEDROCK_LINK`, `GET_XUID_URL`, `GET_GAMERTAG_URL`, `LINK_INFO_URL`.
pub const GET_BEDROCK_LINK: &str = "https://api.geysermc.org/v2/link/bedrock/";
pub const GET_XUID_URL: &str = "https://api.geysermc.org/v2/xbox/xuid/";
pub const GET_GAMERTAG_URL: &str = "https://api.geysermc.org/v2/xbox/gamertag/";
pub const LINK_INFO_URL: &str = "https://link.geysermc.org/";

/// `player-link` in Floodgate's config.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct LinkConfig {
    pub enabled: bool,
    pub require_link: bool,
    pub enable_own_linking: bool,
    pub allowed: bool,
    pub link_code_timeout: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub enable_global_linking: bool,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_link: false,
            enable_own_linking: false,
            allowed: true,
            link_code_timeout: 300,
            kind: "sqlite".into(),
            enable_global_linking: true,
        }
    }
}

/// Whether the Global Linking API is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linking {
    /// `DisabledPlayerLink`.
    Disabled,
    /// `GlobalPlayerLinking` without a database implementation.
    Global,
}

impl LinkConfig {
    pub fn linking(&self) -> Linking {
        if self.enabled && self.enable_global_linking {
            Linking::Global
        } else {
            Linking::Disabled
        }
    }
    /// Own linking is on: the link database answers first and the commands use it.
    pub fn own(&self) -> bool {
        self.enabled && self.enable_own_linking
    }
    /// `LinkAccountCommand.shouldRegister` (the same for `/unlinkaccount`).
    pub fn register_commands(&self) -> bool {
        self.enabled && (self.enable_own_linking || self.enable_global_linking)
    }
}

/// The disconnect message for `require-link`. Floodgate's own text (`floodgate.core.not_linked`) is in its
/// translation files, which have no licence, so this wording is the port's.
pub fn not_linked_message() -> String {
    format!(
        "You need to link your Bedrock account to a Java account to play on this server. See {LINK_INFO_URL}"
    )
}

/// An HTTP answer: the status code (-1 when there was none) and the body.
pub struct HttpResponse {
    pub code: i32,
    pub body: String,
}

impl HttpResponse {
    /// `HttpResponse.isCodeOk`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.code)
    }
    fn json(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        match serde_json::from_str(&self.body) {
            Ok(serde_json::Value::Object(o)) => Some(o),
            _ => None,
        }
    }
    fn message(&self) -> String {
        self.json()
            .and_then(|o| o.get("message")?.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {}", self.code))
    }
}

/// `HttpClient.get`: GET with Floodgate's user agent and timeouts (3 s to connect, 5 s to read).
pub fn http_get(url: &str) -> HttpResponse {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(5))
        .user_agent("GeyserMC/Floodgate")
        .build();
    match agent.get(&url.replace(' ', "%20")).call() {
        Ok(r) => HttpResponse {
            code: r.status() as i32,
            body: r.into_string().unwrap_or_default(),
        },
        Err(ureq::Error::Status(code, r)) => HttpResponse {
            code: code as i32,
            body: r.into_string().unwrap_or_default(),
        },
        Err(_) => HttpResponse {
            code: -1,
            body: String::new(),
        },
    }
}

/// `GlobalPlayerLinking.getLinkedPlayer0` on an answer of `GET_BEDROCK_LINK + xuid`: the link, `Ok(None)` for a
/// player who isn't linked, `Err` with the line Floodgate logs when the API failed.
pub fn parse_link(xuid: i64, r: &HttpResponse) -> Result<Option<LinkedPlayer>, String> {
    if !r.ok() {
        return Err(format!(
            "Failed to request link for {xuid}: {}",
            r.message()
        ));
    }
    let Some(o) = r.json() else {
        return Err(format!("Failed to request link for {xuid}: not JSON"));
    };
    let Some(java_name) = o.get("java_name").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let bad = || format!("Failed to request link for {xuid}: malformed answer");
    let java_uuid = o
        .get("java_id")
        .and_then(|v| v.as_str())
        .and_then(|s| parse_uuid(s).ok())
        .ok_or_else(bad)?;
    let bedrock = o
        .get("bedrock_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(bad)?;
    Ok(Some(LinkedPlayer {
        java_username: java_name.into(),
        java_uuid,
        bedrock_id: u128::from(bedrock as u64),
    }))
}

/// The Global Linking lookup for a Bedrock player (blocking; Floodgate runs it before the login goes on too).
pub fn fetch_global_link(xuid: i64) -> Result<Option<LinkedPlayer>, String> {
    parse_link(xuid, &http_get(&format!("{GET_BEDROCK_LINK}{xuid}")))
}

/// `getXuidFor` on the API's answer.
pub fn parse_xuid(r: &HttpResponse) -> Result<Option<i64>, String> {
    if !r.ok() {
        return Err(r.message());
    }
    Ok(r.json().and_then(|o| o.get("xuid")?.as_i64()))
}

/// `getGamertagFor` on the API's answer.
pub fn parse_gamertag(r: &HttpResponse) -> Result<Option<String>, String> {
    if !r.ok() {
        return Err(r.message());
    }
    Ok(r.json()
        .and_then(|o| o.get("gamertag")?.as_str().map(str::to_string)))
}

/// `FloodgateApi.getXuidFor` (blocking).
pub fn xuid_for(gamertag: &str) -> Result<Option<i64>, String> {
    if gamertag.is_empty() || gamertag.chars().count() > 16 {
        return Err("Received an invalid gamertag".into());
    }
    parse_xuid(&http_get(&format!("{GET_XUID_URL}{gamertag}")))
}

/// `FloodgateApi.getGamertagFor` (blocking).
pub fn gamertag_for(xuid: i64) -> Result<Option<String>, String> {
    parse_gamertag(&http_get(&format!("{GET_GAMERTAG_URL}{xuid}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(code: i32, body: &str) -> HttpResponse {
        HttpResponse {
            code,
            body: body.into(),
        }
    }

    #[test]
    fn link_answers() {
        let linked = r(
            200,
            r#"{"bedrock_id":2535400000000001,"java_id":"11111111-2222-3333-4444-555555555555","java_name":"JavaFriend","last_name_update":0}"#,
        );
        assert_eq!(
            parse_link(2535400000000001, &linked),
            Ok(Some(LinkedPlayer {
                java_username: "JavaFriend".into(),
                java_uuid: 0x11111111_2222_3333_4444_555555555555,
                bedrock_id: 2535400000000001,
            }))
        );
        assert_eq!(parse_link(1, &r(200, "{}")), Ok(None));
        assert_eq!(
            parse_link(1, &r(503, r#"{"message":"down"}"#)),
            Err("Failed to request link for 1: down".into())
        );
        assert!(parse_link(1, &r(-1, "")).is_err());
    }

    #[test]
    fn xbox_lookups() {
        assert_eq!(
            parse_xuid(&r(200, r#"{"xuid":2535400000000001}"#)),
            Ok(Some(2535400000000001))
        );
        assert_eq!(parse_xuid(&r(200, "{}")), Ok(None));
        assert_eq!(
            parse_gamertag(&r(200, r#"{"gamertag":"Steve Two"}"#)),
            Ok(Some("Steve Two".into()))
        );
        assert_eq!(
            parse_gamertag(&r(400, r#"{"message":"bad"}"#)),
            Err("bad".into())
        );
        assert!(xuid_for("").is_err());
        assert!(xuid_for("abcdefghijklmnopq").is_err());
    }

    #[test]
    fn which_linking() {
        let mut c = LinkConfig::default();
        assert_eq!(c.linking(), Linking::Global);
        assert!(c.register_commands());
        c.enable_global_linking = false;
        assert_eq!(c.linking(), Linking::Disabled);
        assert!(!c.own());
        c.enable_own_linking = true;
        assert!(c.register_commands() && c.own());
        c.enabled = false;
        assert!(!c.register_commands());
        let parsed: LinkConfig = toml::from_str("require-link = true\ntype = \"mysql\"").unwrap();
        assert!(parsed.require_link && parsed.enabled);
        assert_eq!(parsed.kind, "mysql");
    }
}
