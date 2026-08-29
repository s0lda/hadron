use regex::Regex;

pub struct InjectionRule {
    pub pattern: Regex,
    pub response: String,
}

pub struct PtyInjector {
    rules: Vec<InjectionRule>,
}

impl PtyInjector {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, pattern: &str, response: &str) {
        if let Ok(re) = Regex::new(pattern) {
            self.rules.push(InjectionRule {
                pattern: re,
                response: response.to_string(),
            });
        }
    }

    pub fn eval_chunk(&self, chunk: &str) -> Option<String> {
        for rule in &self.rules {
            if rule.pattern.is_match(chunk) {
                return Some(rule.response.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_trigger_matching() {
        let mut injector = PtyInjector::new();
        injector.add_rule(r"Proceed\? \[y/N\]", "y\n");

        let response = injector.eval_chunk("Do you want to continue? Proceed? [y/N] ");
        assert_eq!(response, Some("y\n".to_string()));

        let no_response = injector.eval_chunk("Compiling hadron-gluon v0.20.0...");
        assert_eq!(no_response, None);
    }
}
