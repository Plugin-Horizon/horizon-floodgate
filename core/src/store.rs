//! Own linking (`enable-own-linking`): the link database and the pending link requests, after GeyserMC's
//! `LocalPlayerLinking` and its SQLite implementation (MIT, see LICENSE). Only SQLite is ported; `type = "mysql"`
//! falls back to it with a warning. The table layout is a guess at Floodgate's (UUIDs as 16 big-endian bytes).

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::data::LinkedPlayer;

/// The database file inside [`crate::CONFIG_DIR`].
pub const DB_FILE: &str = "linked-players.db";

/// The linked accounts.
pub struct Store {
    conn: rusqlite::Connection,
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

fn id(bytes: Vec<u8>) -> u128 {
    bytes
        .try_into()
        .map(u128::from_be_bytes)
        .unwrap_or_default()
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        Store::from(rusqlite::Connection::open(path).map_err(err)?)
    }

    pub fn open_memory() -> Result<Store, String> {
        Store::from(rusqlite::Connection::open_in_memory().map_err(err)?)
    }

    fn from(conn: rusqlite::Connection) -> Result<Store, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS LinkedPlayers (
                bedrockId BLOB PRIMARY KEY,
                javaUniqueId BLOB NOT NULL,
                javaUsername TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS LinkedPlayersJava ON LinkedPlayers (javaUniqueId);",
        )
        .map_err(err)?;
        Ok(Store { conn })
    }

    fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LinkedPlayer> {
        Ok(LinkedPlayer {
            bedrock_id: id(r.get(0)?),
            java_uuid: id(r.get(1)?),
            java_username: r.get(2)?,
        })
    }

    /// `getLinkedPlayer`: the Java account a Bedrock player (their Floodgate id) is linked to.
    pub fn by_bedrock(&self, bedrock_id: u128) -> Result<Option<LinkedPlayer>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT bedrockId, javaUniqueId, javaUsername FROM LinkedPlayers WHERE bedrockId = ?1",
            )
            .map_err(err)?;
        let mut rows = q
            .query_map([bedrock_id.to_be_bytes().to_vec()], Store::row)
            .map_err(err)?;
        rows.next().transpose().map_err(err)
    }

    /// `isLinkedPlayer` for a Java account: any Bedrock player linked to it.
    pub fn by_java(&self, java_uuid: u128) -> Result<Option<LinkedPlayer>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT bedrockId, javaUniqueId, javaUsername FROM LinkedPlayers WHERE javaUniqueId = ?1",
            )
            .map_err(err)?;
        let mut rows = q
            .query_map([java_uuid.to_be_bytes().to_vec()], Store::row)
            .map_err(err)?;
        rows.next().transpose().map_err(err)
    }

    /// `linkPlayer`: replaces an earlier link of the Bedrock player.
    pub fn link(&self, p: &LinkedPlayer) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO LinkedPlayers (bedrockId, javaUniqueId, javaUsername) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    p.bedrock_id.to_be_bytes().to_vec(),
                    p.java_uuid.to_be_bytes().to_vec(),
                    p.java_username
                ],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// `unlinkPlayer` for a Java account: true when a link was removed.
    pub fn unlink_java(&self, java_uuid: u128) -> Result<bool, String> {
        self.conn
            .execute(
                "DELETE FROM LinkedPlayers WHERE javaUniqueId = ?1",
                [java_uuid.to_be_bytes().to_vec()],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// `unlinkPlayer` for a Bedrock player (their Floodgate id).
    pub fn unlink_bedrock(&self, bedrock_id: u128) -> Result<bool, String> {
        self.conn
            .execute(
                "DELETE FROM LinkedPlayers WHERE bedrockId = ?1",
                [bedrock_id.to_be_bytes().to_vec()],
            )
            .map(|n| n > 0)
            .map_err(err)
    }
}

/// A Java player's request, waiting for the Bedrock player to enter its code (`LinkRequest`).
struct Request {
    java_uuid: u128,
    java_name: String,
    bedrock_name: String,
    code: String,
    at: Instant,
}

/// Why a Bedrock player's code did not link.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkError {
    /// No request from that Java name (or one made for another Bedrock name).
    NoRequest,
    WrongCode,
    Expired,
    Store(String),
}

/// The database and the requests, as Floodgate's `LocalPlayerLinking`.
pub struct Own {
    pub store: Store,
    requests: Vec<Request>,
}

impl Own {
    pub fn new(store: Store) -> Own {
        Own {
            store,
            requests: Vec::new(),
        }
    }

    /// `createLinkRequest`: a new request of a Java player for a Bedrock name replaces their earlier one.
    /// `code` is four digits.
    pub fn request(
        &mut self,
        java_uuid: u128,
        java_name: &str,
        bedrock_name: &str,
        code: String,
        now: Instant,
    ) -> String {
        self.requests
            .retain(|r| !r.java_name.eq_ignore_ascii_case(java_name));
        self.requests.push(Request {
            java_uuid,
            java_name: java_name.into(),
            bedrock_name: bedrock_name.into(),
            code: code.clone(),
            at: now,
        });
        code
    }

    /// `verifyLinkRequest`: the Bedrock player `bedrock_name` (Floodgate id `bedrock_id`) enters `code` for
    /// the Java name; on a match the link is stored and the request ends.
    pub fn complete(
        &mut self,
        java_name: &str,
        bedrock_id: u128,
        bedrock_name: &str,
        code: &str,
        now: Instant,
        timeout: Duration,
    ) -> Result<LinkedPlayer, LinkError> {
        self.requests.retain(|r| now.duration_since(r.at) <= timeout || r.java_name.eq_ignore_ascii_case(java_name));
        let i = self
            .requests
            .iter()
            .position(|r| {
                r.java_name.eq_ignore_ascii_case(java_name) && r.bedrock_name.eq_ignore_ascii_case(bedrock_name)
            })
            .ok_or(LinkError::NoRequest)?;
        if now.duration_since(self.requests[i].at) > timeout {
            self.requests.remove(i);
            return Err(LinkError::Expired);
        }
        if self.requests[i].code != code {
            return Err(LinkError::WrongCode);
        }
        let r = self.requests.remove(i);
        let linked = LinkedPlayer {
            java_username: r.java_name,
            java_uuid: r.java_uuid,
            bedrock_id,
        };
        self.store.link(&linked).map_err(LinkError::Store)?;
        Ok(linked)
    }
}

/// Four digits from the system's random source.
pub fn new_code() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 4];
    let _ = ring::rand::SystemRandom::new().fill(&mut b);
    format!("{:04}", u32::from_le_bytes(b) % 10000)
}

static OWN: Mutex<Option<Own>> = Mutex::new(None);

/// The open database (created in [`crate::CONFIG_DIR`] on first use). None, with the reason logged once per
/// call, when it can't be opened.
pub fn with<R>(f: impl FnOnce(&mut Own) -> R) -> Result<R, String> {
    let mut g: MutexGuard<'_, Option<Own>> = OWN.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        let dir = Path::new(crate::CONFIG_DIR);
        let _ = std::fs::create_dir_all(dir);
        *g = Some(Own::new(Store::open(&dir.join(DB_FILE))?));
    }
    Ok(f(g.as_mut().expect("opened above")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> Own {
        Own::new(Store::open_memory().unwrap())
    }

    #[test]
    fn links_are_stored_and_removed() {
        let o = own();
        let p = LinkedPlayer {
            java_username: "Alex".into(),
            java_uuid: 0xAB << 100,
            bedrock_id: 2535400000000001,
        };
        assert_eq!(o.store.by_bedrock(p.bedrock_id), Ok(None));
        o.store.link(&p).unwrap();
        assert_eq!(o.store.by_bedrock(p.bedrock_id), Ok(Some(p.clone())));
        assert_eq!(o.store.by_java(p.java_uuid), Ok(Some(p.clone())));
        let moved = LinkedPlayer {
            java_username: "Alex2".into(),
            ..p.clone()
        };
        o.store.link(&moved).unwrap();
        assert_eq!(o.store.by_bedrock(p.bedrock_id).unwrap().unwrap().java_username, "Alex2");
        assert_eq!(o.store.unlink_bedrock(p.bedrock_id), Ok(true));
        assert_eq!(o.store.unlink_java(p.java_uuid), Ok(false));
    }

    #[test]
    fn a_request_completes_once_with_the_right_code() {
        let mut o = own();
        let t0 = Instant::now();
        let ten = Duration::from_secs(10);
        let code = o.request(7, "Alex", "Steve", "1234".into(), t0);
        assert_eq!(
            o.complete("alex", 99, "Other", &code, t0, ten),
            Err(LinkError::NoRequest),
            "made for another Bedrock name"
        );
        assert_eq!(o.complete("Alex", 99, "steve", "0000", t0, ten), Err(LinkError::WrongCode));
        let linked = o.complete("ALEX", 99, "Steve", "1234", t0, ten).unwrap();
        assert_eq!((linked.java_uuid, linked.bedrock_id), (7, 99));
        assert_eq!(o.store.by_bedrock(99).unwrap().unwrap().java_username, "Alex");
        assert_eq!(o.complete("Alex", 99, "Steve", "1234", t0, ten), Err(LinkError::NoRequest));
    }

    #[test]
    fn a_request_expires() {
        let mut o = own();
        let t0 = Instant::now();
        o.request(7, "Alex", "Steve", "1234".into(), t0);
        let late = t0 + Duration::from_secs(11);
        assert_eq!(
            o.complete("Alex", 99, "Steve", "1234", late, Duration::from_secs(10)),
            Err(LinkError::Expired)
        );
        assert_eq!(o.store.by_bedrock(99), Ok(None));
    }

    #[test]
    fn codes_are_four_digits() {
        for _ in 0..20 {
            let c = new_code();
            assert!(c.len() == 4 && c.bytes().all(|b| b.is_ascii_digit()), "{c}");
        }
    }
}
