//! Floodgate's data cipher: AES/GCM/NoPadding, a 12-byte IV and a 128-bit tag, with the IV and the
//! cipher text each Base64-encoded. Layout (GeyserMC's `AesCipher` with `Base64Topping`):
//! `"^Floodgate^" + (char)(0x3E + version)` then `base64(iv)` `'!'` `base64(ciphertext || tag)`.
//! Structure taken from GeyserMC's Floodgate (MIT), see LICENSE.

use base64::alphabet::STANDARD;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM, AES_256_GCM, NONCE_LEN};

/// The data version this port reads (`FloodgateCipher.VERSION`).
pub const VERSION: i32 = 0;
/// `FloodgateCipher.IDENTIFIER`.
pub const IDENTIFIER: &[u8] = b"^Floodgate^";
/// `FloodgateCipher.HEADER`: the identifier and the version as one character.
pub const HEADER: &[u8] = b"^Floodgate^>";

/// Java's basic decoder: padding is optional.
const B64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Why a decryption failed. `InvalidFormat` is Floodgate's `InvalidFormatException` (not Floodgate data
/// at all); `Failed` is every other error (a wrong key, tampered data), which Floodgate kicks for.
#[derive(Debug, PartialEq, Eq)]
pub enum DecryptError {
    InvalidFormat,
    Failed,
}

/// `FloodgateCipher.version`: the data version of one host part, or `None` when it isn't Floodgate data.
pub fn version(data: &str) -> Option<i32> {
    let units: Vec<u16> = data.encode_utf16().collect();
    if units.len() <= HEADER.len() {
        return None;
    }
    if IDENTIFIER
        .iter()
        .zip(&units)
        .any(|(a, b)| u16::from(*a) != *b)
    {
        return None;
    }
    Some(i32::from(units[IDENTIFIER.len()]) - 0x3E)
}

/// The key the key file holds: its raw bytes (Floodgate's `AesKeyProducer.produceFrom`).
pub struct Key(LessSafeKey);

impl Key {
    /// A 16-byte (AES-128, what Floodgate generates) or 32-byte (AES-256) key.
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        let algorithm = match bytes.len() {
            16 => &AES_128_GCM,
            32 => &AES_256_GCM,
            n => return Err(format!("the key is {n} bytes, expected 16 or 32")),
        };
        let key = UnboundKey::new(algorithm, bytes).map_err(|_| "bad key".to_string())?;
        Ok(Self(LessSafeKey::new(key)))
    }

    /// `AesCipher.decrypt`.
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, DecryptError> {
        if data.len() <= HEADER.len() || !data.starts_with(IDENTIFIER) {
            return Err(DecryptError::InvalidFormat);
        }
        let body = &data[HEADER.len()..];
        let split = body
            .iter()
            .position(|b| *b == 0x21)
            .ok_or(DecryptError::Failed)?;
        let iv = B64
            .decode(&body[..split])
            .map_err(|_| DecryptError::Failed)?;
        let mut text = B64
            .decode(&body[split + 1..])
            .map_err(|_| DecryptError::Failed)?;
        let nonce = Nonce::try_assume_unique_for_key(&iv).map_err(|_| DecryptError::Failed)?;
        let plain = self
            .0
            .open_in_place(nonce, Aad::empty(), &mut text)
            .map_err(|_| DecryptError::Failed)?;
        Ok(plain.to_vec())
    }

    /// `AesCipher.encrypt` with a random IV: what Geyser and a Floodgate proxy send.
    pub fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut iv = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut iv)
            .map_err(|_| "no random IV".to_string())?;
        let mut text = plain.to_vec();
        self.0
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut text)
            .map_err(|_| "encryption failed".to_string())?;
        let mut out = HEADER.to_vec();
        out.extend_from_slice(B64.encode(iv).as_bytes());
        out.push(0x21);
        out.extend_from_slice(B64.encode(text).as_bytes());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_of_host_parts() {
        assert_eq!(version("^Floodgate^>abc"), Some(0));
        assert_eq!(version("^Floodgate^?abc"), Some(1));
        assert_eq!(version("^Floodgate^>"), None, "no longer than the header");
        assert_eq!(version("example.org"), None);
        assert_eq!(version("^floodgate^>abc"), None);
    }

    #[test]
    fn round_trip_and_wrong_key() {
        let key = Key::new(&[7u8; 16]).unwrap();
        let sealed = key.encrypt(b"hello").unwrap();
        assert!(sealed.starts_with(HEADER));
        assert_eq!(key.decrypt(&sealed).unwrap(), b"hello");
        let other = Key::new(&[8u8; 16]).unwrap();
        assert_eq!(other.decrypt(&sealed), Err(DecryptError::Failed));
    }

    #[test]
    fn malformed_data() {
        let key = Key::new(&[7u8; 16]).unwrap();
        assert_eq!(key.decrypt(b"nope"), Err(DecryptError::InvalidFormat));
        assert_eq!(
            key.decrypt(b"^Floodgate^>noseparator"),
            Err(DecryptError::Failed)
        );
        assert_eq!(
            key.decrypt(b"^Floodgate^>###!###"),
            Err(DecryptError::Failed)
        );
        assert!(Key::new(&[1u8; 24]).is_err());
    }
}
