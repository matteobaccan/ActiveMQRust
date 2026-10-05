// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Credential checks for OpenWire users and the admin console.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::config::{Secret, User};

/// Compares two byte strings in time that depends only on their lengths.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        diff |= x ^ y;
    }
    diff == 0
}

/// Verifies a password against a configured secret.
pub fn verify(secret: &Secret, password: &str) -> bool {
    match secret {
        Secret::Plain(p) => constant_time_eq(p.as_bytes(), password.as_bytes()),
        Secret::Hash(h) => match PasswordHash::new(h) {
            Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
            Err(_) => false,
        },
    }
}

/// Produces an Argon2id hash for `hash-password`.
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Result of an OpenWire login attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    Accepted,
    Rejected,
}

/// OpenWire users plus the anonymous policy.
pub struct Authenticator {
    users: Vec<User>,
    allow_anonymous: bool,
}

impl Authenticator {
    pub fn new(users: Vec<User>, allow_anonymous: bool) -> Self {
        Authenticator { users, allow_anonymous }
    }

    pub fn login(&self, user: Option<&str>, password: Option<&str>) -> Login {
        let user = user.unwrap_or("");
        if user.is_empty() {
            return if self.allow_anonymous { Login::Accepted } else { Login::Rejected };
        }
        let password = password.unwrap_or("");
        // Always run one verification so unknown users cost the same as wrong passwords.
        match self.users.iter().find(|u| u.username == user) {
            Some(u) if verify(&u.secret, password) => Login::Accepted,
            Some(_) => Login::Rejected,
            None => {
                let _ = constant_time_eq(password.as_bytes(), b"--------");
                Login::Rejected
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(anon: bool) -> Authenticator {
        let hash = hash_password("secret").unwrap();
        Authenticator::new(
            vec![
                User { username: "a".into(), secret: Secret::Plain("pw".into()) },
                User { username: "h".into(), secret: Secret::Hash(hash) },
            ],
            anon,
        )
    }

    #[test]
    fn plain_and_hash() {
        let a = auth(false);
        assert_eq!(a.login(Some("a"), Some("pw")), Login::Accepted);
        assert_eq!(a.login(Some("a"), Some("bad")), Login::Rejected);
        assert_eq!(a.login(Some("h"), Some("secret")), Login::Accepted);
        assert_eq!(a.login(Some("h"), Some("nope")), Login::Rejected);
        assert_eq!(a.login(Some("x"), Some("pw")), Login::Rejected);
    }

    #[test]
    fn anonymous_policy() {
        assert_eq!(auth(false).login(None, None), Login::Rejected);
        assert_eq!(auth(true).login(None, None), Login::Accepted);
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
