use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use super::mailbox::{ActorMailbox, QuarkMessage, SwarmEvent};

#[derive(Clone)]
pub struct ActorBus {
    mailboxes: Arc<RwLock<HashMap<String, mpsc::Sender<QuarkMessage>>>>,
    event_tx: broadcast::Sender<SwarmEvent>,
    capacity: usize,
}

impl ActorBus {
    pub fn new(capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(capacity * 4);
        Self {
            mailboxes: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            capacity,
        }
    }

    pub async fn register_quark(
        &self,
        quark_id: &str,
    ) -> anyhow::Result<(ActorMailbox, mpsc::Receiver<QuarkMessage>)> {
        let (tx, rx) = mpsc::channel(self.capacity);
        let mut map = self.mailboxes.write().await;
        map.insert(quark_id.to_string(), tx.clone());
        Ok((
            ActorMailbox {
                quark_id: quark_id.to_string(),
                sender: tx,
            },
            rx,
        ))
    }

    pub async fn unregister_quark(&self, quark_id: &str) {
        let mut map = self.mailboxes.write().await;
        map.remove(quark_id);
    }

    pub async fn active_quarks(&self) -> Vec<String> {
        let map = self.mailboxes.read().await;
        let mut quarks: Vec<String> = map.keys().cloned().collect();
        quarks.sort();
        quarks
    }

    pub async fn send_to_quark(&self, quark_id: &str, msg: QuarkMessage) -> anyhow::Result<()> {
        let sender = {
            let map = self.mailboxes.read().await;
            map.get(quark_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Quark '{quark_id}' not found on ActorBus"))?
        };
        sender.send(msg).await.map_err(|e| anyhow::anyhow!("Failed to send to quark: {e}"))?;
        Ok(())
    }

    pub async fn broadcast_event(&self, event: SwarmEvent) -> anyhow::Result<()> {
        let _ = self.event_tx.send(event);
        Ok(())
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SwarmEvent> {
        self.event_tx.subscribe()
    }
}
