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
