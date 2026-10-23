//! The control channel's session token: upstream's channel needs one (#2764). The window uses
//! `FILMCRAFT_CONTROL_TOKEN` from its environment when it is valid, else a random one of its own;
//! `automation.json` publishes it to local clients (`bashcut/agent-connect.md`).

/// The environment variable upstream's control server and bridge read the token from.
pub const ENV: &str = "FILMCRAFT_CONTROL_TOKEN";

/// A token is 64 hex digits (32 bytes).
pub fn valid(t: &str) -> bool {
    t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())
}

/// A valid `env` token wins, else the generated one.
pub fn choose(env: Option<&str>, generate: impl FnOnce() -> Option<String>) -> Option<String> {
    match env {
        Some(t) if valid(t) => Some(t.to_owned()),
        _ => generate(),
    }
}

/// 32 random bytes as lowercase hex; `None` when the OS gives no randomness (or on wasm).
#[cfg(not(target_arch = "wasm32"))]
pub fn generate() -> Option<String> {
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    let mut out = String::with_capacity(64);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    Some(out)
}

/// No randomness source on wasm: no token.
#[cfg(target_arch = "wasm32")]
pub fn generate() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "0123456789abcdefABCDEF0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn valid_is_64_hex() {
        assert_eq!(GOOD.len(), 64);
        assert!(valid(GOOD));
        assert!(valid(&"a".repeat(64)));
        assert!(!valid(""));
        assert!(!valid(&"a".repeat(63)), "too short");
        assert!(!valid(&"a".repeat(65)), "too long");
        assert!(!valid(&format!("{}g", "a".repeat(63))), "non-hex digit");
        assert!(!valid(&format!("{} ", "a".repeat(63))), "space");
        assert!(!valid(&format!("{}é", "a".repeat(62))), "multi-byte char of the right byte length");
    }

    #[test]
    fn choose_prefers_valid_env() {
        let env = "b".repeat(64);
        let got = choose(Some(&env), || panic!("a valid env token needs no generated one"));
        assert_eq!(got.as_deref(), Some(env.as_str()));
        for bad in ["", "short", &"z".repeat(64)] {
            assert_eq!(choose(Some(bad), || Some("c".repeat(64))), Some("c".repeat(64)), "{bad:?} is ignored");
        }
        assert_eq!(choose(None, || Some("d".repeat(64))), Some("d".repeat(64)));
        assert_eq!(choose(None, || None), None, "no env and no randomness: no token");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn generate_is_random_hex() {
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert!(valid(&a) && valid(&b));
        assert_eq!(a, a.to_ascii_lowercase());
        assert_ne!(a, b);
    }
}
