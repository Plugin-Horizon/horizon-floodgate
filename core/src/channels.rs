//! Floodgate's plugin channels on a backend server (GeyserMC's `FormChannel`, `PacketChannel`, `SkinChannel`,
//! `TransferChannel` and `SpigotPluginMessageRegistration`, MIT, see LICENSE). The proxy's Floodgate passes
//! what the backend sends on them to Geyser, and the Bedrock player's form answers back.
//!
//! The game half calls `on_payload` for a `floodgate:*` payload from a player and `flush` once a tick; `flush`
//! gives what to send and whom to disconnect. Other mods call `send_form`, `close_form`, `send_transfer` and
//! `send_packet`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use crate::forms::{parse_response, Form, Response};

pub const FORM: &str = "floodgate:form";
pub const PACKET: &str = "floodgate:packet";
pub const SKIN: &str = "floodgate:skin";
pub const TRANSFER: &str = "floodgate:transfer";
/// The order Floodgate registers its channels in (`PluginMessageRegister`).
pub const CHANNELS: [&str; 4] = [FORM, PACKET, SKIN, TRANSFER];

/// `minecraft:register` with the channels, each followed by a NUL (what Paper sends a player for the channels a
/// plugin listens on, as observed for another plugin's channels).
pub fn register_payload() -> Vec<u8> {
    CHANNELS.iter().flat_map(|c| c.bytes().chain([0])).collect()
}

/// A player's answer to a form: their UUID and the answer.
pub type Callback = Box<dyn FnOnce(u128, Response) + Send>;

/// `FormChannel.createFormData`: the form type, the form id (big-endian), the JSON.
pub fn form_data(form: &Form, id: i16) -> Vec<u8> {
    let json = form.to_json();
    let mut data = Vec::with_capacity(json.len() + 3);
    data.push(form.form_type() as u8);
    data.extend_from_slice(&id.to_be_bytes());
    data.extend_from_slice(json.as_bytes());
    data
}

/// `TransferChannel.sendTransfer`: the port as a big-endian int, then the address.
pub fn transfer_data(address: &str, port: i32) -> Vec<u8> {
    let mut data = port.to_be_bytes().to_vec();
    data.extend_from_slice(address.as_bytes());
    data
}

/// A skin Geyser uploaded for a player: the texture value and its signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skin {
    pub value: String,
    pub signature: String,
}

/// `SkinChannel.handleServerCall`: `value\0signature`, exactly two parts (Java's split).
pub fn parse_skin(data: &[u8]) -> Option<Skin> {
    let text = String::from_utf8_lossy(data);
    let parts = crate::data::java_split(&text, '\0');
    match parts[..] {
        [value, signature] => Some(Skin {
            value: value.into(),
            signature: signature.into(),
        }),
        _ => None,
    }
}

/// What the game half does after `flush`.
#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Payload {
        player: u128,
        channel: &'static str,
        data: Vec<u8>,
    },
    Kick {
        player: u128,
        reason: String,
    },
    Log {
        level: &'static str,
        line: String,
    },
    /// Put this skin into the player's profile and show them again (`SkinApplier`).
    ApplySkin {
        player: u128,
        skin: Skin,
    },
}

struct Active {
    form: Form,
    callback: Option<Callback>,
}

#[derive(Default)]
struct PerPlayer {
    next_form_id: i32,
    forms: HashMap<i16, Active>,
    /// `PropertyKey.SKIN_UPLOADED`.
    skin: Option<Skin>,
}

/// The channels' state: per player, the forms waiting for an answer; the messages to send.
#[derive(Default)]
pub struct Channels {
    players: HashMap<u128, PerPlayer>,
    seen: HashSet<u128>,
    out: Vec<Out>,
    /// Answers to run outside the lock.
    answers: Vec<(Callback, u128, Response)>,
}

impl Channels {
    /// `FormChannel.getNextFormId`: 0 up to `i16::MAX`, then 0 again. A backend never sets the top bit (that
    /// is the proxy's own forms).
    fn next_form_id(&mut self, player: u128) -> i16 {
        let p = self.players.entry(player).or_default();
        let id = p.next_form_id;
        p.next_form_id = if id == i16::MAX as i32 { 0 } else { id + 1 };
        id as i16
    }

    pub fn send_form(&mut self, player: u128, form: Form, callback: Option<Callback>) {
        let id = self.next_form_id(player);
        let data = form_data(&form, id);
        self.players
            .entry(player)
            .or_default()
            .forms
            .insert(id, Active { form, callback });
        self.out.push(Out::Payload {
            player,
            channel: FORM,
            data,
        });
    }

    /// `FormChannel.closeForms0`: each open form is answered as closed.
    fn close_all(&mut self, player: u128) {
        if let Some(p) = self.players.get_mut(&player) {
            for (_, active) in p.forms.drain() {
                let response = parse_response(&active.form, "");
                if let Some(cb) = active.callback {
                    self.answers.push((cb, player, response));
                }
            }
        }
    }

    /// `FormChannel.closeForm`: closes the player's forms and tells the client (an empty payload).
    pub fn close_form(&mut self, player: u128) {
        self.close_all(player);
        self.out.push(Out::Payload {
            player,
            channel: FORM,
            data: Vec::new(),
        });
    }

    pub fn send_transfer(&mut self, player: u128, address: &str, port: i32) {
        self.out.push(Out::Payload {
            player,
            channel: TRANSFER,
            data: transfer_data(address, port),
        });
    }

    /// `PacketChannel.sendPacket` (Floodgate's unsafe API): raw Bedrock packet data for Geyser.
    pub fn send_packet(&mut self, player: u128, packet: Vec<u8>) {
        self.out.push(Out::Payload {
            player,
            channel: PACKET,
            data: packet,
        });
    }

    fn kick(&mut self, player: u128, reason: &str) {
        self.out.push(Out::Kick {
            player,
            reason: reason.into(),
        });
    }

    /// A payload from a player on a `floodgate:` channel. `name` is the player's (correct) name for the log,
    /// `None` when they are not a Floodgate player.
    pub fn on_payload(&mut self, player: u128, name: Option<&str>, channel: &str, data: &[u8]) {
        if !CHANNELS.contains(&channel) {
            return;
        }
        let Some(name) = name else {
            self.kick(
                player,
                "Only Floodgate players can send floodgate messages!",
            );
            return;
        };
        match channel {
            FORM => {
                // `callResponseConsumer`.
                let stored = (data.len() >= 2)
                    .then(|| i16::from_be_bytes([data[0], data[1]]))
                    .and_then(|id| self.players.get_mut(&player)?.forms.remove(&id));
                match stored {
                    Some(active) => {
                        let text = String::from_utf8_lossy(&data[2..]);
                        let response = parse_response(&active.form, &text);
                        if let Some(cb) = active.callback {
                            self.answers.push((cb, player, response));
                        }
                    }
                    None => self.out.push(Out::Log {
                        level: "ERROR",
                        line: format!("[floodgate] Couldn't find stored form for player {name}"),
                    }),
                }
            }
            PACKET => self.kick(
                player,
                "Cannot send packets from Geyser/Floodgate to Floodgate",
            ),
            TRANSFER => self.kick(player, "I'm sorry, I'm unable to transfer a server :("),
            SKIN => match parse_skin(data) {
                Some(skin) => {
                    // Floodgate applies it to the online player here (`SkinApplier`): the glue does it with
                    // hooks-all's native_set_profile_property and native_refresh_player.
                    self.players.entry(player).or_default().skin = Some(skin.clone());
                    self.out.push(Out::ApplySkin { player, skin });
                }
                None => self.kick(player, "Got invalid skin data"),
            },
            _ => {}
        }
    }

    /// The skin Geyser uploaded for a player while they were on this server.
    pub fn uploaded_skin(&self, player: u128) -> Option<Skin> {
        self.players.get(&player)?.skin.clone()
    }

    /// The player left: their forms are answered as closed (`FormChannel.disconnect`).
    pub fn player_left(&mut self, player: u128) {
        self.close_all(player);
        self.players.remove(&player);
        self.seen.remove(&player);
    }

    /// Once a tick with the players who are placed: new ones get `minecraft:register`, then everything queued.
    fn flush(&mut self, online: &[u128]) -> (Vec<Out>, Vec<(Callback, u128, Response)>) {
        let mut out = Vec::new();
        for &p in online {
            if self.seen.insert(p) {
                out.push(Out::Payload {
                    player: p,
                    channel: "minecraft:register",
                    data: register_payload(),
                });
            }
        }
        out.append(&mut self.out);
        (out, std::mem::take(&mut self.answers))
    }
}

static CHANNELS_STATE: Mutex<Option<Channels>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Channels>> {
    CHANNELS_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn with<R>(f: impl FnOnce(&mut Channels) -> R) -> R {
    let mut g = lock();
    f(g.get_or_insert_with(Channels::default))
}

/// Runs the answers' callbacks without holding the lock (a callback may send the next form).
fn answer(answers: Vec<(Callback, u128, Response)>) {
    for (cb, player, response) in answers {
        cb(player, response);
    }
}

/// `FloodgateApi.sendForm`: does nothing for a player who is not a Floodgate player, as Floodgate.
pub fn send_form(player: u128, form: Form, callback: Option<Callback>) -> bool {
    if crate::player(player).is_some() {
        with(|c| c.send_form(player, form, callback));
    }
    true
}

/// `FloodgateApi.closeForm`.
pub fn close_form(player: u128) -> bool {
    if crate::player(player).is_some() {
        let answers = with(|c| {
            c.close_form(player);
            std::mem::take(&mut c.answers)
        });
        answer(answers);
    }
    true
}

/// `FloodgateApi.transferPlayer`: Geyser sends the Bedrock client to another Bedrock server.
pub fn send_transfer(player: u128, address: &str, port: i32) -> bool {
    with(|c| c.send_transfer(player, address, port));
    true
}

/// Disconnects a player with `reason` at the next tick (the game half does it).
pub fn kick(player: u128, reason: &str) {
    with(|c| c.kick(player, reason));
}

/// `UnsafeFloodgateApi.sendPacket`.
pub fn send_packet(player: u128, packet: Vec<u8>) -> bool {
    with(|c| c.send_packet(player, packet));
    true
}

/// The skin Geyser uploaded for a player after they joined, if any.
pub fn uploaded_skin(player: u128) -> Option<Skin> {
    lock().as_ref()?.uploaded_skin(player)
}

/// The game half: a payload from a player on a `floodgate:` channel.
pub fn on_payload(player: u128, channel: &str, data: &[u8]) {
    let name = crate::player(player).map(|p| p.correct_username().to_string());
    let answers = with(|c| {
        c.on_payload(player, name.as_deref(), channel, data);
        std::mem::take(&mut c.answers)
    });
    answer(answers);
}

/// The game half: the player left.
pub fn player_left(player: u128) {
    let answers = with(|c| {
        c.player_left(player);
        std::mem::take(&mut c.answers)
    });
    answer(answers);
}

/// The game half, once a tick, with the UUIDs of the placed players: what to send and whom to disconnect.
pub fn flush(online: &[u128]) -> Vec<Out> {
    let (out, answers) = with(|c| c.flush(online));
    answer(answers);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::Button;
    use std::sync::{Arc, Mutex as M};

    fn modal() -> Form {
        Form::Modal {
            title: "t".into(),
            content: "c".into(),
            button1: "a".into(),
            button2: "b".into(),
        }
    }

    #[test]
    fn data_formats() {
        let d = form_data(&modal(), 0x0102);
        assert_eq!(&d[..3], &[1, 1, 2]);
        assert!(d[3..].starts_with(b"{"));
        assert_eq!(
            transfer_data("bedrock.example", 19132),
            [&[0, 0, 0x4a, 0xbc][..], b"bedrock.example"].concat()
        );
        assert_eq!(
            parse_skin(b"val\0sig"),
            Some(Skin {
                value: "val".into(),
                signature: "sig".into()
            })
        );
        assert_eq!(parse_skin(b"val"), None);
        assert_eq!(parse_skin(b"a\0b\0c"), None);
        assert_eq!(
            register_payload(),
            b"floodgate:form\0floodgate:packet\0floodgate:skin\0floodgate:transfer\0"
        );
    }

    #[test]
    fn a_form_is_answered_once_then_forgotten() {
        let got = Arc::new(M::new(Vec::new()));
        let mut c = Channels::default();
        let g = got.clone();
        let form = Form::Simple {
            title: "t".into(),
            content: "".into(),
            buttons: vec![Some(Button::new("x")), Some(Button::new("y"))],
        };
        c.send_form(
            7,
            form,
            Some(Box::new(move |p: u128, r: Response| {
                g.lock().unwrap().push((p, r))
            })),
        );
        let (out, _) = c.flush(&[7]);
        assert!(matches!(
            &out[0],
            Out::Payload {
                channel: "minecraft:register",
                ..
            }
        ));
        let Out::Payload {
            channel: FORM,
            data,
            ..
        } = &out[1]
        else {
            panic!()
        };
        assert_eq!(&data[..3], &[0, 0, 0]);
        c.on_payload(7, Some("p"), FORM, &[0, 0, b'1']);
        for (cb, p, r) in std::mem::take(&mut c.answers) {
            cb(p, r);
        }
        assert_eq!(
            got.lock().unwrap()[..],
            [(
                7,
                Response::Simple {
                    clicked: 1,
                    text: "y".into()
                }
            )]
        );
        c.on_payload(7, Some("p"), FORM, &[0, 0, b'1']);
        assert!(matches!(
            &c.flush(&[7]).0[..],
            [Out::Log { level: "ERROR", .. }]
        ));
    }

    #[test]
    fn form_ids_wrap_and_leaving_closes() {
        let mut c = Channels::default();
        c.players.entry(1).or_default().next_form_id = i16::MAX as i32;
        assert_eq!(c.next_form_id(1), i16::MAX);
        assert_eq!(c.next_form_id(1), 0);
        let closed = Arc::new(M::new(None));
        let k = closed.clone();
        c.send_form(
            1,
            modal(),
            Some(Box::new(move |_: u128, r: Response| {
                *k.lock().unwrap() = Some(r)
            })),
        );
        c.player_left(1);
        for (cb, p, r) in std::mem::take(&mut c.answers) {
            cb(p, r);
        }
        assert_eq!(*closed.lock().unwrap(), Some(Response::Closed));
    }

    #[test]
    fn refusals_kick_like_floodgate() {
        let mut c = Channels::default();
        c.on_payload(2, None, FORM, &[0, 0]);
        c.on_payload(3, Some("b"), PACKET, &[1]);
        c.on_payload(3, Some("b"), TRANSFER, &[1]);
        c.on_payload(3, Some("b"), SKIN, b"only-one-part");
        c.on_payload(3, Some("b"), "other:channel", &[]);
        let reasons: Vec<String> = c
            .flush(&[])
            .0
            .into_iter()
            .filter_map(|o| match o {
                Out::Kick { reason, .. } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(
            reasons,
            [
                "Only Floodgate players can send floodgate messages!",
                "Cannot send packets from Geyser/Floodgate to Floodgate",
                "I'm sorry, I'm unable to transfer a server :(",
                "Got invalid skin data"
            ]
        );
        c.on_payload(3, Some("b"), SKIN, b"v\0s");
        assert_eq!(c.uploaded_skin(3).unwrap().value, "v");
        let applied: Vec<Out> = c
            .flush(&[3])
            .0
            .into_iter()
            .filter(|o| matches!(o, Out::ApplySkin { .. }))
            .collect();
        assert_eq!(
            applied,
            [Out::ApplySkin {
                player: 3,
                skin: Skin {
                    value: "v".into(),
                    signature: "s".into()
                }
            }]
        );
    }
}
