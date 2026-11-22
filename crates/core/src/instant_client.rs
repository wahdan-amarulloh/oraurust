//! Oracle Instant Client check (ADR-0001).
//!
//! The `oracle` crate (ODPI-C) needs the Oracle Instant Client shared
//! library (`oci.dll` on Windows) on the machine. This module turns a missing
//! or incompatible client into a readable, actionable error instead of a
//! panic, and offers a cheap startup probe so the app can warn early.

use crate::connection::DbError;
use std::path::PathBuf;

/// Result of probing the machine for Oracle Instant Client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientStatus {
    /// `oci.dll` was found in this directory.
    Found(PathBuf),
    /// No search path held `oci.dll`.
    Missing,
}

/// The shared library ODPI-C loads.
#[cfg(windows)]
const OCI_LIBRARY: &str = "oci.dll";
#[cfg(not(windows))]
const OCI_LIBRARY: &str = "libclntsh.so";

/// Probes each directory on `search_paths` for the Oracle client library.
///
/// The caller passes the directories worth checking — `PATH` entries, the
/// `ORACLE_HOME` bin directory, a bundled client folder — rather than this
/// module reading the environment itself, so the probe stays testable.
pub fn check(search_paths: &[PathBuf]) -> ClientStatus {
    search_paths
        .iter()
        .find(|dir| dir.join(OCI_LIBRARY).is_file())
        .map(|dir| ClientStatus::Found(dir.clone()))
        .unwrap_or(ClientStatus::Missing)
}

/// The remediation text shown when Oracle Instant Client is missing: what to
/// install and where to get it (issue #5 acceptance criterion).
pub fn remediation() -> String {
    "Oracle Instant Client (Basic or Basic Light, 64-bit) is required. \
     Download it from https://www.oracle.com/database/technologies/instant-client/downloads.html, \
     unzip it to a permanent folder, and add that folder to PATH (or set ORACLE_HOME). \
     The app needs version 11.2 or later to match the 11g driver."
        .to_string()
}

/// Maps an ODPI-C "client library" error (missing or incompatible Oracle
/// Instant Client) to a [`DbError`] carrying the remediation text.
///
/// Returns `None` for every other error, so ordinary `ORA-` failures pass
/// through untouched.
pub fn classify_error(error: DbError) -> Option<DbError> {
    let is_client_problem = matches!(error.code.as_str(), "DPI-1047" | "DPI-1072");
    if !is_client_problem {
        return None;
    }

    let headline = if error.code == "DPI-1072" {
        "The installed Oracle Instant Client version is not compatible with this app."
    } else {
        "Oracle Instant Client was not found."
    };
    Some(DbError {
        code: error.code,
        message: format!("{headline} {}", remediation()),
        position: None,
    })
}
