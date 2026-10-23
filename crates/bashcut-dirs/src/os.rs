//! The one OS mapping (RC5, S5): plugin manifests, the registry, the data folder and the doctor all ask
//! [`Os`], so FreeBSD and friends land on `linux` everywhere and a new OS is one edit here.

use std::fmt;

/// The OS families BashCutPro tells apart; every Unix that is not macOS counts as `Linux` (XDG
/// folders, the `linux` manifest entry).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    /// Every family, in manifest order.
    pub const ALL: [Os; 3] = [Os::Macos, Os::Windows, Os::Linux];

    /// This build's family.
    pub fn current() -> Os {
        Os::from_name(std::env::consts::OS)
    }

    /// A `std::env::consts::OS` value or a manifest name: `macos`, `windows`, anything else `Linux`.
    pub fn from_name(name: &str) -> Os {
        match name {
            "macos" => Os::Macos,
            "windows" => Os::Windows,
            _ => Os::Linux,
        }
    }

    /// The name manifests and registries use (`platforms`, the `entry` keys).
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Macos => "macos",
            Os::Windows => "windows",
            Os::Linux => "linux",
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Os;

    #[test]
    fn every_other_unix_is_linux() {
        assert_eq!(Os::from_name("macos"), Os::Macos);
        assert_eq!(Os::from_name("windows"), Os::Windows);
        for other in ["linux", "freebsd", "openbsd", "netbsd", "dragonfly", "solaris", "illumos", "android", ""] {
            assert_eq!(Os::from_name(other), Os::Linux, "{other:?}");
        }
        for os in Os::ALL {
            assert_eq!(Os::from_name(os.as_str()), os);
            assert_eq!(os.to_string(), os.as_str());
        }
        assert_eq!(Os::current(), Os::from_name(std::env::consts::OS));
    }
}
