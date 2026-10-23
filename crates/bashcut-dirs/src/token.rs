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
    #[test]
    #[ignore = "test ticket: valid accepts 64 hex digits, rejects other lengths and non-hex"]
    fn valid_is_64_hex() {}

    #[test]
    #[ignore = "test ticket: choose prefers a valid env token and ignores an invalid one"]
    fn choose_prefers_valid_env() {}

    #[test]
    #[ignore = "test ticket: generate gives 64 hex digits and two calls differ"]
    fn generate_is_random_hex() {}
}
