//! Connection profiles: a named, saved profile (host, port, service/SID,
//! credentials) — GLOSSARY.md's Connection entry. The profile is the data;
//! the live session it opens is the `Connection` seam.

use std::fmt;

/// A named, saved connection profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Service name or SID, as the user typed it (Easy Connect path).
    pub service: String,
    pub username: String,
    /// Decrypted only in memory; never written to disk in the clear.
    pub password: String,
}

impl Profile {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        service: impl Into<String>,
        username: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            port,
            service: service.into(),
            username: username.into(),
            password: String::new(),
        }
    }

    /// Easy Connect string for the `oracle` crate:
    /// `//host:port/service`.
    pub fn connect_string(&self) -> String {
        format!("//{}:{}/{}", self.host, self.port, self.service)
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.username)
    }
}
