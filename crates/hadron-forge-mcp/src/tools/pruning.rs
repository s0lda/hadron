use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Core,
    Diagnostics,
    Debugger,
    Swarm,
    Nucleus,
    Profiling,
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFilter {
    pub categories: HashSet<ToolCategory>,
}

impl ToolFilter {
    pub fn all() -> Self {
        let mut categories = HashSet::new();
        categories.insert(ToolCategory::Core);
        categories.insert(ToolCategory::Diagnostics);
        categories.insert(ToolCategory::Debugger);
        categories.insert(ToolCategory::Swarm);
        categories.insert(ToolCategory::Nucleus);
        categories.insert(ToolCategory::Profiling);
        categories.insert(ToolCategory::Web);
        Self { categories }
    }

    pub fn from_skill(skill: &str) -> Self {
        let mut categories = HashSet::new();
        categories.insert(ToolCategory::Core);
        match skill {
            "writing-plans" | "brainstorming" | "writing-skills" => {
                categories.insert(ToolCategory::Nucleus);
                categories.insert(ToolCategory::Diagnostics);
            }
            "systematic-debugging" | "test-driven-development" => {
                categories.insert(ToolCategory::Debugger);
                categories.insert(ToolCategory::Diagnostics);
            }
            "performance-audit" => {
                categories.insert(ToolCategory::Profiling);
                categories.insert(ToolCategory::Diagnostics);
            }
            "dispatching-parallel-agents" | "executing-plans" => {
                categories.insert(ToolCategory::Swarm);
                categories.insert(ToolCategory::Nucleus);
            }
            _ => return Self::all(),
        }
        Self { categories }
    }

    pub fn matches_tool(&self, tool: &str) -> bool {
        if self.categories.contains(&ToolCategory::Core)
            && (tool.contains("read_file")
                || tool.contains("write_file")
                || tool.contains("exec")
                || tool.contains("git_"))
        {
            return true;
        }
        if self.categories.contains(&ToolCategory::Nucleus) && tool.contains("nucleus") {
            return true;
        }
        if self.categories.contains(&ToolCategory::Debugger)
            && (tool.contains("dap") || tool.contains("breakpoint") || tool.contains("vcr"))
        {
            return true;
        }
        if self.categories.contains(&ToolCategory::Profiling)
            && (tool.contains("flamegraph") || tool.contains("profile") || tool.contains("bloat"))
        {
            return true;
        }
        if self.categories.contains(&ToolCategory::Swarm)
            && (tool.contains("peers") || tool.contains("topology") || tool.contains("mesh"))
        {
            return true;
        }
        if self.categories.contains(&ToolCategory::Diagnostics)
            && (tool.contains("diagnostics")
                || tool.contains("symbols")
                || tool.contains("semantic"))
        {
            return true;
        }
        if self.categories.contains(&ToolCategory::Web)
            && (tool.contains("browser") || tool.contains("preview") || tool.contains("screenshot"))
        {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_filter_by_skill() {
        let filter = ToolFilter::from_skill("writing-plans");
        assert!(filter.matches_tool("hadron_forge_read_file"));
        assert!(filter.matches_tool("hadron_forge_nucleus_list"));
        assert!(!filter.matches_tool("hadron_forge_dap_launch"));
        assert!(!filter.matches_tool("hadron_forge_flamegraph"));

        let debug_filter = ToolFilter::from_skill("systematic-debugging");
        assert!(debug_filter.matches_tool("hadron_forge_dap_launch"));
        assert!(debug_filter.matches_tool("hadron_forge_read_file"));
    }
}
