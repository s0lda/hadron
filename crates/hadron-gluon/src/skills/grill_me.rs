use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrillInterviewState {
    pub feature_topic: String,
    pub questions_asked: Vec<String>,
    pub answers_received: Vec<String>,
    pub synthesized_spec: Option<String>,
}

impl GrillInterviewState {
    pub fn new(feature_topic: impl Into<String>) -> Self {
        Self {
            feature_topic: feature_topic.into(),
            questions_asked: Vec::new(),
            answers_received: Vec::new(),
            synthesized_spec: None,
        }
    }

    pub fn ask_question(&mut self, question: impl Into<String>) {
        self.questions_asked.push(question.into());
    }

    pub fn answer_question(&mut self, answer: impl Into<String>) {
        self.answers_received.push(answer.into());
    }

    pub fn pending_question(&self) -> Option<&str> {
        if self.questions_asked.len() > self.answers_received.len() {
            self.questions_asked.get(self.answers_received.len()).map(|s| s.as_str())
        } else {
            None
        }
    }

    pub fn is_complete(&self, min_questions: usize) -> bool {
        self.questions_asked.len() >= min_questions
            && self.questions_asked.len() == self.answers_received.len()
    }

    pub fn synthesize_spec(&mut self) -> String {
        let mut spec = format!(
            "# Specification: {}\n\n*Synthesized via `/grill-me` alignment interview.*\n\n## Requirements & Invariant Decisions\n\n",
            self.feature_topic
        );

        for (idx, (q, a)) in self.questions_asked.iter().zip(&self.answers_received).enumerate() {
            spec.push_str(&format!(
                "### Q{}: {}\n**User Decision:** {}\n\n",
                idx + 1,
                q,
                a
            ));
        }

        self.synthesized_spec = Some(spec.clone());
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grill_interview_state_machine() {
        let mut state = GrillInterviewState::new("Dark Mode Preferences");
        assert!(!state.is_complete(2));

        state.ask_question("Should high-contrast OLED black be supported?");
        assert_eq!(state.pending_question(), Some("Should high-contrast OLED black be supported?"));

        state.answer_question("Yes, provide an OLED True Black theme preset.");
        assert_eq!(state.pending_question(), None);
        assert!(!state.is_complete(2));

        state.ask_question("What is the fallback font behavior?");
        state.answer_question("Fallback directly to system sans without external downloads.");
        assert!(state.is_complete(2));

        let spec = state.synthesize_spec();
        assert!(spec.contains("# Specification: Dark Mode Preferences"));
        assert!(spec.contains("OLED True Black"));
        assert!(spec.contains("system sans"));
    }
}
