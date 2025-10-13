use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

#[derive(Clone)]
pub struct SecretsManager {
    key: Vec<u8>,
}

impl std::fmt::Debug for SecretsManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsManager")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl SecretsManager {
    pub fn new() -> Result<Self> {
        // Load master key from environment
        let key_str = std::env::var("SARAMCP_MASTER_KEY").unwrap_or_else(|_| {
            // Generate a default key for development
            // In production, this should be set in the environment
            tracing::warn!("SARAMCP_MASTER_KEY not set, using default key (INSECURE!)");
            BASE64.encode(&b"ThisIsA32ByteKeyForDevelopmentOnly!!!!!!!!!"[..32])
        });

        let key_bytes = BASE64
            .decode(key_str)
            .map_err(|e| anyhow!("Invalid master key encoding: {}", e))?;

        if key_bytes.len() != 32 {
            return Err(anyhow!("Master key must be exactly 32 bytes"));
        }

        Ok(Self { key: key_bytes })
    }

    pub fn generate_master_key() -> String {
        let mut key = [0u8; 32];
        use rand::RngCore;
        OsRng.fill_bytes(&mut key);
        BASE64.encode(key)
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let key = Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        // Combine nonce and ciphertext
        let mut combined = nonce.to_vec();
        combined.extend_from_slice(&ciphertext);

        Ok(BASE64.encode(&combined))
    }

    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        let combined = BASE64
            .decode(encrypted)
            .map_err(|e| anyhow!("Invalid encrypted data encoding: {}", e))?;

        if combined.len() < 12 {
            return Err(anyhow!("Invalid encrypted data: too short"));
        }

        // Split nonce and ciphertext
        let (nonce, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce);

        let key = Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        String::from_utf8(plaintext).map_err(|e| anyhow!("Invalid UTF-8 in decrypted data: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let secrets = SecretsManager::new().unwrap();
        let plaintext = "This is a secret password!";

        let encrypted = secrets.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);

        let decrypted = secrets.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_encryptions() {
        let secrets = SecretsManager::new().unwrap();
        let plaintext = "Same text";

        let encrypted1 = secrets.encrypt(plaintext).unwrap();
        let encrypted2 = secrets.encrypt(plaintext).unwrap();

        // Different nonces should produce different ciphertexts
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same plaintext
        assert_eq!(secrets.decrypt(&encrypted1).unwrap(), plaintext);
        assert_eq!(secrets.decrypt(&encrypted2).unwrap(), plaintext);
    }

    #[test]
    fn test_invalid_encrypted_data() {
        let secrets = SecretsManager::new().unwrap();

        // Invalid base64
        assert!(secrets.decrypt("not-base64!@#").is_err());

        // Too short
        assert!(secrets.decrypt("dGVzdA==").is_err());

        // Invalid ciphertext
        let invalid = BASE64.encode(vec![0u8; 20]);
        assert!(secrets.decrypt(&invalid).is_err());
    }
}
