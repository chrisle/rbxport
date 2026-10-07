//! `SQLCipher` key derivation.
//!
//! rekordbox stores the database passphrase encrypted in its agent's
//! `options.json` under `dp`. The value is base64, and decrypts with Blowfish
//! in ECB mode under a fixed key.
//!
//! Verified against this machine's rekordbox 7.2.11 install.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use blowfish::Blowfish;
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit};

use crate::{DbError, Result};

/// The fixed Blowfish key rekordbox uses to protect the passphrase.
const MAGIC: &[u8] = b"ZOwUlUZYqe9Rdm6j";

const BLOCK: usize = 8;

/// rekordbox's wrapped passphrase, as its agent writes it into `options.json`
/// under `dp`. Byte-for-byte the same on macOS 7.2.11 and Windows 7.2.14, and
/// in a library rekordbox 7.2.14 made afresh [OBS 2026-09-24], so a library
/// found without an `options.json` beside it — on a drive, or on a machine
/// rekordbox is not installed on — is opened with it.
pub const REKORDBOX_DP: &str =
    "FJ9s0iA+hiPZgURNVQNg+Aj/UQ41IlitwloFsPnU3sISVHn5EVNQwthYGuUdAryEcCzJZHnZ5Q7JoupTY9FDRw==";

/// Decrypts the `dp` field from `options.json` into the `SQLCipher` passphrase.
///
/// Note there is **no PKCS padding** on rekordbox 6/7 databases: the plaintext
/// is trailing-trimmed rather than unpadded. Assuming PKCS5 (as some published
/// recipes do) corrupts the last block.
pub fn derive_password(dp_base64: &str) -> Result<String> {
    let ciphertext = STANDARD
        .decode(dp_base64.trim())
        .map_err(|e| DbError::KeyDerivation(format!("dp is not valid base64: {e}")))?;

    if ciphertext.is_empty() || ciphertext.len() % BLOCK != 0 {
        return Err(DbError::KeyDerivation(format!(
            "dp decodes to {} bytes, which is not a whole number of Blowfish blocks",
            ciphertext.len()
        )));
    }

    let cipher = Blowfish::<byteorder::BigEndian>::new_from_slice(MAGIC)
        .map_err(|e| DbError::KeyDerivation(format!("bad Blowfish key: {e}")))?;

    let mut plaintext = ciphertext.clone();
    for block in plaintext.chunks_exact_mut(BLOCK) {
        cipher.decrypt_block(block.into());
    }

    // Trailing bytes are whitespace/NUL, not PKCS padding.
    let end = plaintext
        .iter()
        .rposition(|b| !b.is_ascii_whitespace() && *b != 0)
        .map_or(0, |i| i + 1);
    plaintext.truncate(end);

    String::from_utf8(plaintext)
        .map_err(|e| DbError::KeyDerivation(format!("passphrase is not UTF-8: {e}")))
}

/// The inverse: wraps a passphrase the way rekordbox's agent does, so a
/// fixture can carry an `options.json` the detector reads like the real one.
///
/// NUL-padded to a whole block, which `derive_password` trims off again.
pub fn wrap_password(passphrase: &str) -> Result<String> {
    let cipher = Blowfish::<byteorder::BigEndian>::new_from_slice(MAGIC)
        .map_err(|e| DbError::KeyDerivation(format!("bad Blowfish key: {e}")))?;
    let mut plaintext = passphrase.as_bytes().to_vec();
    // At least one block: an empty passphrase still has to decode to
    // something `derive_password` accepts.
    let short = plaintext.len() % BLOCK;
    if short != 0 || plaintext.is_empty() {
        plaintext.resize(plaintext.len() + BLOCK - short, 0);
    }
    for block in plaintext.chunks_exact_mut(BLOCK) {
        cipher.encrypt_block(block.into());
    }
    Ok(STANDARD.encode(&plaintext))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapped_passphrase_derives_back_whatever_its_length() {
        for secret in ["a", "rbxport-fixture", "exactly eight bytes!!!!!", ""] {
            let dp = wrap_password(secret).unwrap();
            assert_eq!(derive_password(&dp).unwrap(), secret, "{secret:?}");
        }
    }

    /// Round-trips through our own encryptor so the test needs no real secret.
    #[test]
    fn decrypts_what_the_same_cipher_encrypted() {
        let cipher = Blowfish::<byteorder::BigEndian>::new_from_slice(MAGIC).unwrap();
        let secret = b"correct horse battery staple!!!!"; // 32 bytes, block aligned
        let mut buf = secret.to_vec();
        for block in buf.chunks_exact_mut(BLOCK) {
            cipher.encrypt_block(block.into());
        }
        let dp = STANDARD.encode(&buf);
        assert_eq!(derive_password(&dp).unwrap(), "correct horse battery staple!!!!");
    }

    #[test]
    fn rejects_input_that_is_not_block_aligned() {
        let dp = STANDARD.encode([1, 2, 3]);
        assert!(matches!(derive_password(&dp), Err(DbError::KeyDerivation(_))));
    }

    #[test]
    fn rejects_non_base64() {
        assert!(matches!(derive_password("not base64!!"), Err(DbError::KeyDerivation(_))));
    }
}
