//! `KeyringStore` — the real `SecretStore` backend, persisting values in the
//! OS credential store via the `keyring` crate (native Keychain on macOS,
//! Credential Manager on Windows, Secret Service over D-Bus on *nix).
//!
//! `team.json` only ever holds var *names* (`Seat.secret_env`); this is the
//! only place actual values are read or written, and they never touch disk
//! in plaintext. See `hadron_lattice::secrets` for the trait and the account
//! key format (`"{seat}/{var}"`) shared with `MemoryStore`.

use hadron_lattice::secrets::{account_key, SecretStore};
use hadron_lattice::QuarkId;

/// The credential-store "service" name under which every seat/var account is
/// filed. One constant so lookups and writes can never drift apart (SSOT).
const SERVICE: &str = "hadron";

/// `SecretStore` backed by the platform credential store (via `keyring`).
///
/// Holds no state of its own beyond the service name — `keyring::Entry` is
/// cheap to construct per call, so there is nothing to cache or lock.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self { service: SERVICE.to_string() }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

// Deliberately NOT `#[derive(Debug)]`: a naive derive is safe here today (the
// struct only holds the service name), but a future field could easily be a
// cached secret. Keep this type opaque so no accidental `{:?}` of it — or of
// anything holding it — can ever print a password. If you need to debug-print
// this type, add a hand-written impl that redacts, the way `MemoryStore` does.

impl SecretStore for KeyringStore {
    fn get(&self, seat: &QuarkId, var: &str) -> anyhow::Result<Option<String>> {
        let account = account_key(seat, var);
        let entry = keyring::Entry::new(&self.service, &account)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, seat: &QuarkId, var: &str, value: &str) -> anyhow::Result<()> {
        let account = account_key(seat, var);
        let entry = keyring::Entry::new(&self.service, &account)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn delete(&self, seat: &QuarkId, var: &str) -> anyhow::Result<()> {
        let account = account_key(seat, var);
        let entry = keyring::Entry::new(&self.service, &account)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_hadron() {
        // Pins the SSOT service name so a future edit here doesn't silently
        // orphan every credential already filed under "hadron".
        assert_eq!(KeyringStore::new().service, "hadron");
    }

    /// Live-only round trip against a real OS credential store. Ignored by
    /// default: a headless/CI run has no Secret Service session (WSL2
    /// included), so `Entry::new`/`get_password` would just error out.
    /// Run manually with a desktop session available:
    ///   cargo test -p hadron-gluon secrets::tests::live_round_trip -- --ignored
    #[test]
    #[ignore = "needs a live OS credential store (Secret Service/Keychain/Credential Manager)"]
    fn live_round_trip() {
        let store = KeyringStore::new();
        let seat = QuarkId::new("test-seat-keyring-live");
        let var = "TEST_VAR";

        store.set(&seat, var, "live-secret-value").unwrap();
        assert_eq!(
            store.get(&seat, var).unwrap(),
            Some("live-secret-value".to_string())
        );
        store.delete(&seat, var).unwrap();
        assert_eq!(store.get(&seat, var).unwrap(), None);
    }
}
