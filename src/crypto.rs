use blake3::Hasher as Blake3;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::random;

/// Load or generate a SigningKey from a hex-encoded secret.
///
/// The secret can be:
/// - A 32-byte hex string (with or without 0x prefix)
/// - An arbitrary-length hex string (will be hashed to 32 bytes)
/// - None (will generate a new random key)
///
/// Returns both the signing key and the hex-encoded secret for persistence.
pub fn load_or_generate_signing_key(secret: Option<String>) -> (SigningKey, String) {
    if let Some(secret_hex) = secret {
        if let Ok(decoded) = hex::decode(secret_hex.trim_start_matches("0x")) {
            if decoded.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&decoded);
                let key = SigningKey::from_bytes(&seed);
                return (key, format!("0x{}", hex::encode(seed)));
            }
            // fallback: hash arbitrary input to 32 bytes
            let mut hasher = Blake3::new();
            hasher.update(&decoded);
            let digest = hasher.finalize();
            let seed: [u8; 32] = *digest.as_bytes();
            let key = SigningKey::from_bytes(&seed);
            return (key, format!("0x{}", hex::encode(seed)));
        }
    }
    // Generate new random seed
    let seed: [u8; 32] = random();
    let key = SigningKey::from_bytes(&seed);
    let secret_hex = format!("0x{}", hex::encode(seed));
    (key, secret_hex)
}

/// Decode a hex-encoded secret into a SigningKey.
/// Returns None if the secret is invalid.
pub fn signing_key_from_hex(secret_hex: &str) -> Option<SigningKey> {
    let decoded = hex::decode(secret_hex.trim_start_matches("0x")).ok()?;
    if decoded.len() == 32 {
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&decoded);
        return Some(SigningKey::from_bytes(&seed));
    }
    None
}

/// Generate a new random SigningKey and return it with its hex-encoded secret.
pub fn generate_signing_key() -> (SigningKey, String) {
    let seed: [u8; 32] = random();
    let key = SigningKey::from_bytes(&seed);
    let secret_hex = format!("0x{}", hex::encode(seed));
    (key, secret_hex)
}

/// Get the verifying key (public key) from a signing key.
pub fn verifying_key_from_signing(signing_key: &SigningKey) -> VerifyingKey {
    signing_key.verifying_key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_or_generate_with_valid_hex() {
        let secret = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let (key1, hex1) = load_or_generate_signing_key(Some(secret.to_string()));
        let (key2, hex2) = load_or_generate_signing_key(Some(secret.to_string()));

        // Same secret should produce same key
        assert_eq!(hex1, hex2);
        assert_eq!(key1.to_bytes(), key2.to_bytes());
    }

    #[test]
    fn test_load_or_generate_creates_random() {
        let (key1, hex1) = load_or_generate_signing_key(None);
        let (key2, hex2) = load_or_generate_signing_key(None);

        // Random keys should be different
        assert_ne!(hex1, hex2);
        assert_ne!(key1.to_bytes(), key2.to_bytes());
    }

    #[test]
    fn test_signing_key_from_hex() {
        let secret = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key = signing_key_from_hex(secret);
        assert!(key.is_some());
    }

    #[test]
    fn test_generate_signing_key() {
        let (key1, hex1) = generate_signing_key();
        let (key2, hex2) = generate_signing_key();

        // Each generation should produce different keys
        assert_ne!(hex1, hex2);
        assert_ne!(key1.to_bytes(), key2.to_bytes());
    }

    #[test]
    fn test_verifying_key_from_signing() {
        let (signing_key, _) = generate_signing_key();
        let verifying_key = verifying_key_from_signing(&signing_key);

        // Should match the signing key's verifying key
        assert_eq!(
            verifying_key.as_bytes(),
            signing_key.verifying_key().as_bytes()
        );
    }
}
