use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Hash a setup token using argon2
pub fn hash_token(token: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash error: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a setup token against its hash (constant-time)
pub fn verify_token(token: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("parse hash error: {}", e))?;
    Ok(Argon2::default()
        .verify_password(token.as_bytes(), &parsed)
        .is_ok())
}

/// Generate a random setup token (32 bytes, hex encoded)
pub fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let token = generate_token();
        let hash = hash_token(&token).unwrap();
        assert!(verify_token(&token, &hash).unwrap());
        assert!(!verify_token("wrong-token", &hash).unwrap());
    }
}
