use std::time::{Duration, Instant};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: usize,
    pub kind: ToastKind,
    pub message: String,
    pub created_at: Instant,
    pub duration: Duration,
}

impl Toast {
    pub fn new(id: usize, kind: ToastKind, message: impl Into<String>, duration_secs: Option<u64>) -> Self {
        Self {
            id,
            kind,
            message: message.into(),
            created_at: Instant::now(),
            duration: Duration::from_secs(duration_secs.unwrap_or(4)),
        }
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.created_at) >= self.duration
    }
}

#[derive(Debug, Default)]
pub struct ToastManager {
    toasts: Vec<Toast>,
    next_id: usize,
}

impl ToastManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, kind: ToastKind, message: impl Into<String>, duration_secs: Option<u64>) -> usize {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.toasts.len() >= 5 {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast::new(id, kind, message, duration_secs));
        id
    }

    pub fn dismiss(&mut self, id: usize) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.id != id);
        before != self.toasts.len()
    }

    pub fn prune(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| !t.is_expired(now));
        before != self.toasts.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.toasts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_manager_lifecycle() {
        let mut mgr = ToastManager::new();
        assert!(mgr.is_empty());

        let id1 = mgr.push(ToastKind::Success, "Operation succeeded", Some(10));
        let _id2 = mgr.push(ToastKind::Info, "FYI", Some(10));
        assert_eq!(mgr.len(), 2);

        assert!(mgr.dismiss(id1));
        assert_eq!(mgr.len(), 1);
        assert!(!mgr.dismiss(id1)); // already gone

        // Expiration check
        let mut expired_mgr = ToastManager::new();
        expired_mgr.push(ToastKind::Warning, "Short warning", Some(0));
        std::thread::sleep(Duration::from_millis(5));
        assert!(expired_mgr.prune(Instant::now()));
        assert!(expired_mgr.is_empty());
    }

    #[test]
    fn test_toast_manager_max_capacity() {
        let mut mgr = ToastManager::new();
        for i in 0..10 {
            mgr.push(ToastKind::Info, format!("Msg {i}"), Some(10));
        }
        assert_eq!(mgr.len(), 5);
        assert_eq!(mgr.iter().last().unwrap().message, "Msg 9");
    }
}
