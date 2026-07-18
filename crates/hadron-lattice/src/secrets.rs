use crate::QuarkId;

/// Where per-seat secret env-var VALUES live. Values are keyed by (seat id, var
/// name); the account string is `"{seat}/{var}"` under a single service `"hadron"`.
/// Implementors persist to a real credential store (OS keychain) or, in tests, to
/// memory. NO implementation writes values to `team.json` or any plaintext file.
pub trait SecretStore: Send + Sync {
    /// The stored value for (seat, var), or `None` if unset. `Err` only on a
    /// backend failure (e.g. no credential service) — an absent key is `Ok(None)`.
    fn get(&self, seat: &QuarkId, var: &str) -> anyhow::Result<Option<String>>;
    /// Store (or overwrite) the value for (seat, var).
    fn set(&self, seat: &QuarkId, var: &str, value: &str) -> anyhow::Result<()>;
    /// Remove (seat, var); removing an absent key is Ok (idempotent).
    fn delete(&self, seat: &QuarkId, var: &str) -> anyhow::Result<()>;
}

/// The account key a backend uses for (seat, var): `"{seat}/{var}"`. Public so the
/// keyring impl and tests share ONE format (SSOT).
pub fn account_key(seat: &QuarkId, var: &str) -> String {
    format!("{}/{}", seat.as_str(), var)
}

/// In-memory `SecretStore` for tests — NEVER touches a real keychain. Interior
/// mutability so it can be shared behind `&`.
#[derive(Default)]
pub struct MemoryStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

// Values are secrets: never let a derived/auto Debug print them. This impl
// redacts by construction — it only ever shows the key count.
impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.inner.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("MemoryStore")
            .field("entries", &len)
            .finish()
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, seat: &QuarkId, var: &str) -> anyhow::Result<Option<String>> {
        let key = account_key(seat, var);
        let map = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("MemoryStore mutex poisoned"))?;
        Ok(map.get(&key).cloned())
    }

    fn set(&self, seat: &QuarkId, var: &str, value: &str) -> anyhow::Result<()> {
        let key = account_key(seat, var);
        let mut map = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("MemoryStore mutex poisoned"))?;
        map.insert(key, value.to_string());
        Ok(())
    }

    fn delete(&self, seat: &QuarkId, var: &str) -> anyhow::Result<()> {
        let key = account_key(seat, var);
        let mut map = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("MemoryStore mutex poisoned"))?;
        map.remove(&key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_key_is_seat_slash_var() {
        assert_eq!(
            account_key(&QuarkId::new("acp-agy"), "GEMINI_API_KEY"),
            "acp-agy/GEMINI_API_KEY"
        );
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::new();
        let seat = QuarkId::new("acp-agy");
        store.set(&seat, "GEMINI_API_KEY", "secret-value").unwrap();
        assert_eq!(
            store.get(&seat, "GEMINI_API_KEY").unwrap(),
            Some("secret-value".to_string())
        );
    }

    #[test]
    fn memory_store_get_absent_is_none() {
        let store = MemoryStore::new();
        let seat = QuarkId::new("acp-agy");
        assert_eq!(store.get(&seat, "GEMINI_API_KEY").unwrap(), None);
    }

    #[test]
    fn memory_store_isolates_by_seat_and_var() {
        let store = MemoryStore::new();
        let seat_a = QuarkId::new("seat-a");
        let seat_b = QuarkId::new("seat-b");
        store.set(&seat_a, "VAR", "x").unwrap();
        store.set(&seat_b, "VAR", "y").unwrap();
        store.set(&seat_a, "OTHER", "z").unwrap();

        assert_eq!(store.get(&seat_a, "VAR").unwrap(), Some("x".to_string()));
        assert_eq!(store.get(&seat_b, "VAR").unwrap(), Some("y".to_string()));
        assert_eq!(store.get(&seat_a, "OTHER").unwrap(), Some("z".to_string()));
        assert_eq!(store.get(&seat_b, "OTHER").unwrap(), None);
    }

    #[test]
    fn memory_store_delete_is_idempotent() {
        let store = MemoryStore::new();
        let seat = QuarkId::new("acp-agy");

        // Deleting an absent key is Ok.
        store.delete(&seat, "GEMINI_API_KEY").unwrap();

        store.set(&seat, "GEMINI_API_KEY", "secret-value").unwrap();
        store.delete(&seat, "GEMINI_API_KEY").unwrap();
        assert_eq!(store.get(&seat, "GEMINI_API_KEY").unwrap(), None);
    }

    #[test]
    fn memory_store_overwrite() {
        let store = MemoryStore::new();
        let seat = QuarkId::new("acp-agy");
        store.set(&seat, "VAR", "first").unwrap();
        store.set(&seat, "VAR", "second").unwrap();
        assert_eq!(store.get(&seat, "VAR").unwrap(), Some("second".to_string()));
    }
}
