//! The decrypted Bedrock data (GeyserMC's `BedrockData` and `LinkedPlayer`, MIT, see LICENSE): twelve
//! fields separated by NUL, in this order.

/// `BedrockData.EXPECTED_LENGTH`.
pub const EXPECTED_LENGTH: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedPlayer {
    pub java_username: String,
    pub java_uuid: u128,
    pub bedrock_id: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BedrockData {
    pub version: String,
    pub username: String,
    pub xuid: String,
    pub device_os: i32,
    pub language_code: String,
    pub ui_profile: i32,
    pub input_mode: i32,
    pub ip: String,
    pub linked_player: Option<LinkedPlayer>,
    pub from_proxy: bool,
    pub subscribe_id: i32,
    pub verify_code: String,
}

/// Why the data could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Not twelve fields: Floodgate's `INVALID_DATA_LENGTH`.
    Length(usize),
    /// A number or UUID that doesn't parse: Floodgate's `EXCEPTION`.
    Field(String),
}

/// Java's `String.split` with a one-character separator: trailing empty parts are dropped.
pub fn java_split(s: &str, sep: char) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts == [""] && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `Integer.parseInt`.
fn int(s: &str) -> Result<i32, ParseError> {
    s.parse::<i32>()
        .map_err(|_| ParseError::Field(format!("not a number: {s:?}")))
}

/// `UUID.fromString` for the canonical form (the only one Floodgate writes).
pub fn parse_uuid(s: &str) -> Result<u128, ParseError> {
    let bad = || ParseError::Field(format!("not a UUID: {s:?}"));
    let b = s.as_bytes();
    if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
        return Err(bad());
    }
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    u128::from_str_radix(&hex, 16).map_err(|_| bad())
}

pub fn uuid_string(v: u128) -> String {
    let h = format!("{v:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

impl LinkedPlayer {
    /// `LinkedPlayer.fromString`: `name;javaUuid;bedrockUuid`, or `None` for anything without three parts
    /// (the literal `null` included).
    fn parse(s: &str) -> Result<Option<Self>, ParseError> {
        let parts = java_split(s, ';');
        if parts.len() != 3 {
            return Ok(None);
        }
        Ok(Some(Self {
            java_username: parts[0].to_string(),
            java_uuid: parse_uuid(parts[1])?,
            bedrock_id: parse_uuid(parts[2])?,
        }))
    }
}

impl BedrockData {
    /// `BedrockData.fromString`.
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let p = java_split(s, '\0');
        if p.len() != EXPECTED_LENGTH {
            return Err(ParseError::Length(p.len()));
        }
        Ok(Self {
            version: p[0].to_string(),
            username: p[1].to_string(),
            xuid: p[2].to_string(),
            device_os: int(p[3])?,
            language_code: p[4].to_string(),
            ui_profile: int(p[5])?,
            input_mode: int(p[6])?,
            ip: p[7].to_string(),
            linked_player: LinkedPlayer::parse(p[8])?,
            from_proxy: p[9] == "1",
            subscribe_id: int(p[10])?,
            verify_code: p[11].to_string(),
        })
    }

    /// `BedrockData.toString`: the inverse of `parse`.
    pub fn to_wire(&self) -> String {
        let linked = match &self.linked_player {
            Some(l) => format!(
                "{};{};{}",
                l.java_username,
                uuid_string(l.java_uuid),
                uuid_string(l.bedrock_id)
            ),
            None => "null".to_string(),
        };
        [
            self.version.clone(),
            self.username.clone(),
            self.xuid.clone(),
            self.device_os.to_string(),
            self.language_code.clone(),
            self.ui_profile.to_string(),
            self.input_mode.to_string(),
            self.ip.clone(),
            linked,
            if self.from_proxy { "1" } else { "0" }.to_string(),
            self.subscribe_id.to_string(),
            self.verify_code.clone(),
        ]
        .join("\0")
    }

    /// `Utils.getJavaUuid(xuid)`: `new UUID(0, Long.parseLong(xuid))`.
    pub fn java_uuid(&self) -> Result<u128, ParseError> {
        let xuid = self
            .xuid
            .parse::<i64>()
            .map_err(|_| ParseError::Field(format!("not a XUID: {:?}", self.xuid)))?;
        Ok(u128::from(xuid as u64))
    }

    /// `HandshakeDataImpl`'s Java name: the prefix and the Bedrock name cut to 16 characters in all,
    /// spaces made underscores when `replace_spaces` is on.
    pub fn java_username(&self, prefix: &str, replace_spaces: bool) -> String {
        let room = 16usize.saturating_sub(prefix.chars().count());
        let mut name: String = prefix.to_string();
        name.extend(self.username.chars().take(room));
        if replace_spaces {
            name = name.replace(' ', "_");
        }
        name
    }
}

/// `DeviceOs.fromId(id).toString()`.
pub fn device_os_name(id: i32) -> &'static str {
    const NAMES: [&str; 16] = [
        "Unknown",
        "Android",
        "iOS",
        "macOS",
        "Amazon",
        "Gear VR",
        "Hololens",
        "Windows",
        "Windows x86",
        "Dedicated",
        "Apple TV",
        "PlayStation",
        "Switch",
        "Xbox",
        "Windows Phone",
        "Linux",
    ];
    usize::try_from(id)
        .ok()
        .and_then(|i| NAMES.get(i))
        .unwrap_or(&NAMES[0])
}

/// `InputMode.fromId(id)`.
pub fn input_mode_name(id: i32) -> &'static str {
    const NAMES: [&str; 5] = ["UNKNOWN", "KEYBOARD_MOUSE", "TOUCH", "CONTROLLER", "VR"];
    usize::try_from(id)
        .ok()
        .and_then(|i| NAMES.get(i))
        .unwrap_or(&NAMES[0])
}

/// `UiProfile.fromId(id)`.
pub fn ui_profile_name(id: i32) -> &'static str {
    if id == 1 {
        "POCKET"
    } else {
        "CLASSIC"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn enum_names() {
        assert_eq!(device_os_name(7), "Windows");
        assert_eq!(device_os_name(99), "Unknown");
        assert_eq!(input_mode_name(3), "CONTROLLER");
        assert_eq!(ui_profile_name(1), "POCKET");
    }

    use super::*;

    fn sample() -> BedrockData {
        BedrockData {
            version: "1.21.130".into(),
            username: "Steve Two".into(),
            xuid: "2535400000000001".into(),
            device_os: 7,
            language_code: "en_US".into(),
            ui_profile: 0,
            input_mode: 1,
            ip: "203.0.113.9".into(),
            linked_player: None,
            from_proxy: true,
            subscribe_id: 42,
            verify_code: "abc".into(),
        }
    }

    #[test]
    fn round_trip() {
        let d = sample();
        assert_eq!(BedrockData::parse(&d.to_wire()).unwrap(), d);
        let linked = BedrockData {
            linked_player: Some(LinkedPlayer {
                java_username: "JavaName".into(),
                java_uuid: 0x1234,
                bedrock_id: 0x0009_01ab_cdef_0001,
            }),
            ..sample()
        };
        assert_eq!(BedrockData::parse(&linked.to_wire()).unwrap(), linked);
    }

    #[test]
    fn wrong_lengths_and_fields() {
        assert_eq!(BedrockData::parse("a\0b"), Err(ParseError::Length(2)));
        // Java's split drops a trailing empty field: an empty verify code makes eleven.
        let d = BedrockData {
            verify_code: String::new(),
            ..sample()
        };
        assert_eq!(
            BedrockData::parse(&d.to_wire()),
            Err(ParseError::Length(11))
        );
        let bad = sample().to_wire().replace("\07\0", "\0x\0");
        assert!(matches!(
            BedrockData::parse(&bad),
            Err(ParseError::Field(_))
        ));
    }

    #[test]
    fn java_identity() {
        let d = sample();
        assert_eq!(d.java_username(".", true), ".Steve_Two");
        assert_eq!(d.java_username(".", false), ".Steve Two");
        let long = BedrockData {
            username: "ABCDEFGHIJKLMNOPQRS".into(),
            ..sample()
        };
        assert_eq!(long.java_username(".", true), ".ABCDEFGHIJKLMNO");
        assert_eq!(
            uuid_string(d.java_uuid().unwrap()),
            "00000000-0000-0000-0009-01eed05d1001"
        );
        assert_eq!(
            parse_uuid("00000000-0000-0000-0009-01eed05d1001").unwrap(),
            d.java_uuid().unwrap()
        );
        assert_eq!(java_split("a;b;;", ';'), vec!["a", "b"]);
    }
}
