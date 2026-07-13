use std::collections::VecDeque;

use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

use crate::quark::Quark;

/// A deterministic quark for tests. Emits scripted messages in order; once the
/// script is exhausted it emits `repeating` (or `None`) on every further turn.
pub struct MockQuark {
    id: QuarkId,
    flavor: Flavor,
    scripted: VecDeque<Option<String>>,
    repeating: Option<String>,
}

impl MockQuark {
    /// Emit each queued message once, in order (`None` = a silent turn).
    pub fn scripted(id: QuarkId, flavor: Flavor, messages: Vec<Option<String>>) -> Self {
        MockQuark {
            id,
            flavor,
            scripted: messages.into_iter().collect(),
            repeating: None,
        }
    }

    /// Emit the same message on every turn forever (drives backstop tests).
    pub fn repeating(id: QuarkId, flavor: Flavor, message: impl Into<String>) -> Self {
        MockQuark {
            id,
            flavor,
            scripted: VecDeque::new(),
            repeating: Some(message.into()),
        }
    }
}

#[async_trait]
impl Quark for MockQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
        let message = self
            .scripted
            .pop_front()
            .unwrap_or_else(|| self.repeating.clone());
        Ok(TurnOutcome { message, permission: None, usage: Default::default() })
    }
}
