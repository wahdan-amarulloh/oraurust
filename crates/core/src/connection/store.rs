//! `ProfileStore`: named profiles persisted to one JSON file, with the
//! password encrypted at rest (never plaintext in the profile file — issue #5
//! acceptance criteria).
//!
//! On Windows the password is sealed with DPAPI (`CryptProtectData`,
//! current-user scope, `CRYPTPROTECT_UI_FORBIDDEN`), so the ciphertext only
//! unseals under the same Windows account — no key file to lose. On other
//! platforms (this tool targets Windows, but the core stays portable for
//! tests/CI) it falls back to base64 with a marker prefix. Either way the
//! profile file itself is JSON with the other fields in the clear.

use super::Profile;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The stored form of a [`Profile`]: everything in the clear except the
/// password, which is an opaque sealed blob.
#[derive(Serialize, Deserialize)]
struct StoredProfile {
    name: String,
    host: String,
    port: u16,
    service: String,
    username: String,
    /// Sealed password (DPAPI blob, base64'd).
    password: String,
}

impl StoredProfile {
    fn seal(profile: &Profile) -> io::Result<Self> {
        Ok(Self {
            name: profile.name.clone(),
            host: profile.host.clone(),
            port: profile.port,
            service: profile.service.clone(),
            username: profile.username.clone(),
            password: seal_password(&profile.password)?,
        })
    }

    fn unseal(self) -> io::Result<Profile> {
        Ok(Profile {
            name: self.name,
            host: self.host,
            port: self.port,
            service: self.service,
            username: self.username,
            password: unseal_password(&self.password)?,
        })
    }
}

/// Prefix marking the fallback (non-DPAPI) encoding used off-Windows.
#[cfg(not(windows))]
const FALLBACK_PREFIX: &str = "b64:";

#[cfg(windows)]
fn seal_password(password: &str) -> io::Result<String> {
    use std::ptr;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    let plain: Vec<u8> = password
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let result = unsafe {
        CryptProtectData(
            &CRYPT_INTEGER_BLOB {
                cbData: plain.len() as u32,
                pbData: plain.as_ptr() as *mut u8,
            },
            null(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let sealed = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    Ok(base64::engine::general_purpose::STANDARD.encode(sealed))
}

#[cfg(windows)]
fn unseal_password(sealed_b64: &str) -> io::Result<String> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let sealed = base64::engine::general_purpose::STANDARD
        .decode(sealed_b64)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let result = unsafe {
        CryptUnprotectData(
            &CRYPT_INTEGER_BLOB {
                cbData: sealed.len() as u32,
                pbData: sealed.as_ptr() as *mut u8,
            },
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let plain = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    let units: Vec<u16> = plain
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16(&units).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(not(windows))]
fn seal_password(password: &str) -> io::Result<String> {
    // Non-Windows stand-in (tests/CI only): base64, clearly marked so we can
    // tell it apart from a DPAPI blob.
    Ok(format!(
        "{FALLBACK_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(password.as_bytes())
    ))
}

#[cfg(not(windows))]
fn unseal_password(sealed: &str) -> io::Result<String> {
    sealed
        .strip_prefix(FALLBACK_PREFIX)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown password encoding"))
        .and_then(|b64| {
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
}

/// Named connection profiles, persisted to one JSON file.
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    /// A store over `path` (typically under the user's config directory).
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Where the profile file lives (tests use it to inspect raw bytes).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Saves `profile`, keyed by its name: a profile with the same name is
    /// edited in place, a new one is appended. Password is sealed before
    /// hitting disk.
    pub fn save(&self, profile: &Profile) -> io::Result<()> {
        let mut profiles = self.read_stored()?;
        match profiles.iter().position(|p| p.name == profile.name) {
            Some(ix) => profiles[ix] = StoredProfile::seal(profile)?,
            None => profiles.push(StoredProfile::seal(profile)?),
        }
        self.write_stored(&profiles)
    }

    /// Loads all profiles, newest-last, with passwords unsealed.
    pub fn load(&self) -> io::Result<Vec<Profile>> {
        self.read_stored()?
            .into_iter()
            .map(StoredProfile::unseal)
            .collect()
    }

    /// Deletes the named profile. Deleting an unknown name is not an error.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        let mut profiles = self.read_stored()?;
        profiles.retain(|p| p.name != name);
        self.write_stored(&profiles)
    }

    fn read_stored(&self) -> io::Result<Vec<StoredProfile>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("corrupt profile file {}: {e}", self.path.display()),
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn write_stored(&self, profiles: &[StoredProfile]) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(profiles)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.path, json)
    }
}
