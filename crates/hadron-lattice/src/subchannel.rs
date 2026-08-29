use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct SubChannelMessage {
    pub sender: String,
    pub content: String,
}

pub struct SubChannel {
    pub id: String,
    pub participants: HashSet<String>,
    pub messages: Vec<SubChannelMessage>,
}

impl SubChannel {
    pub fn new(id: &str, members: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            participants: members.iter().map(|s| s.to_string()).collect(),
            messages: Vec::new(),
        }
    }

    pub fn post_message(&mut self, sender: &str, content: &str) {
        if self.participants.contains(sender) {
            self.messages.push(SubChannelMessage {
                sender: sender.to_string(),
                content: content.to_string(),
            });
        }
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn synthesize_checkpoint(&self) -> String {
        format!(
            "Subchannel {} checkpoint: {} messages exchanged across {} participants.",
            self.id,
            self.messages.len(),
            self.participants.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subchannel_isolation_and_checkpointing() {
        let mut sc = SubChannel::new("sc-42", &["worker", "reviewer"]);
        sc.post_message("worker", "Reviewing AST diff for safety.");
        sc.post_message("reviewer", "Approved. Invariant checks passed.");

        assert_eq!(sc.message_count(), 2);
        let summary = sc.synthesize_checkpoint();
        assert!(summary.contains("2 messages exchanged"));
    }
}
