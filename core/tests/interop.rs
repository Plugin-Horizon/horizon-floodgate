//! Data encrypted by GeyserMC's own `AesCipher` (Floodgate 2.2.5, `tools/Interop.java gen`) with the
//! throwaway test key `test-key.bin`, which protects nothing else.

use horizon_floodgate_core::cipher::Key;
use horizon_floodgate_core::data::{uuid_string, BedrockData, ParseError};
use horizon_floodgate_core::{handle_host, Config, Outcome};

fn key() -> Key {
    Key::new(include_bytes!("test-key.bin")).unwrap()
}

fn cases() -> Vec<(String, String, String)> {
    include_str!("java_vectors.tsv")
        .lines()
        .map(|l| {
            let p: Vec<&str> = l.split('\t').collect();
            (p[1].to_string(), p[2].to_string(), p[3].to_string())
        })
        .collect()
}

/// The getters as `Interop.describe` joins them.
fn describe(d: &BedrockData) -> String {
    let linked = match &d.linked_player {
        Some(l) => format!(
            "{};{};{}",
            l.java_username,
            uuid_string(l.java_uuid),
            uuid_string(l.bedrock_id)
        ),
        None => "null".into(),
    };
    [
        d.version.clone(),
        d.username.clone(),
        d.xuid.clone(),
        d.device_os.to_string(),
        d.language_code.clone(),
        d.ui_profile.to_string(),
        d.input_mode.to_string(),
        d.ip.clone(),
        linked,
        d.from_proxy.to_string(),
        d.subscribe_id.to_string(),
        d.verify_code.clone(),
        "12".into(),
    ]
    .join("|")
}

#[test]
fn reads_what_java_floodgate_writes() {
    let key = key();
    for (label, encrypted, expected) in cases() {
        let plain = key.decrypt(encrypted.as_bytes()).expect(&label);
        let text = String::from_utf8(plain).unwrap();
        match BedrockData::parse(&text) {
            Ok(d) => assert_eq!(describe(&d), expected, "{label}"),
            Err(ParseError::Length(n)) => {
                assert!(expected.ends_with(&format!("|{n}")), "{label}: {expected}")
            }
            Err(e) => panic!("{label}: {e:?}"),
        }
    }
}

#[test]
fn handles_java_hosts() {
    let key = key();
    let config = Config::default();
    let all = cases();
    let host = |label: &str| {
        let enc = &all.iter().find(|c| c.0 == label).unwrap().1;
        format!("play.example.org\0{enc}")
    };
    let Some(Outcome::Bedrock(p)) = handle_host(&host("linked"), &config, Some(&key)) else {
        panic!("linked refused");
    };
    assert_eq!(p.correct_username(), "JavaName");
    assert_eq!(
        uuid_string(p.correct_uuid()),
        "11111111-2222-3333-4444-555555555555"
    );
    assert_eq!(p.java_username, ".LinkedOne");
    let Some(Outcome::Bedrock(p)) = handle_host(&host("plain"), &config, Some(&key)) else {
        panic!("plain refused");
    };
    assert_eq!(p.correct_username(), ".Steve_Two");
    assert_eq!(
        handle_host(&host("short"), &config, Some(&key)),
        Some(Outcome::Kick(
            config.disconnect.invalid_arguments_length.clone()
        ))
    );
}

/// Writes data for `Interop.java read` (Rust encrypts, Java decrypts) when `FLOODGATE_RUST_VECTORS` names
/// a file; does nothing otherwise.
#[test]
fn write_rust_vectors() {
    let Ok(path) = std::env::var("FLOODGATE_RUST_VECTORS") else {
        return;
    };
    let key = key();
    let mut out = String::new();
    for (label, encrypted, _) in cases() {
        let plain = key.decrypt(encrypted.as_bytes()).unwrap();
        let again = String::from_utf8(key.encrypt(&plain).unwrap()).unwrap();
        out.push_str(&format!("case\t{label}\t{again}\n"));
    }
    std::fs::write(path, out).unwrap();
}
