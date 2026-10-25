//! The chamber's segmented-tab enums: which side rails collapse ([`Rail`]) and the
//! three tab strips over the chat column ([`ChatTab`]), the quark info panel
//! ([`InfoTab`]), and the right rail ([`RightRailTab`]). Pure value types — each
//! carries its own `ALL` / `index` / `from_index` / `label` for the tab bars.

/// The two collapsible side rails.
#[derive(Clone, Copy)]
pub(super) enum Rail {
    Roster,
    Inspector,
}

/// The views over the field, selected by the chat column's segmented tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChatTab {
    /// The conversation — human/quark messages, styled like a chat.
    Chat,
    /// Every event on the field, compact — the raw activity log.
    Log,
    /// Team-wide stats over a selectable time window (Session/Week/Month/All time):
    /// turns, tokens, context, quota.
    Stats,
}

impl ChatTab {
    pub(super) const ALL: [ChatTab; 3] = [ChatTab::Chat, ChatTab::Log, ChatTab::Stats];

    /// Sizes every per-tab array. A tab added to `ALL` without growing those
    /// arrays is an index-out-of-bounds the moment the tab is opened, so they
    /// take their length from here rather than restating it.
    #[allow(dead_code)] // arrays currently size from ALL.len() directly
    pub(super) const COUNT: usize = Self::ALL.len();

    pub(super) fn index(self) -> usize {
        match self {
            ChatTab::Chat => 0,
            ChatTab::Log => 1,
            ChatTab::Stats => 2,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(ChatTab::Chat)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            ChatTab::Chat => "Chat",
            ChatTab::Log => "Event Log",
            ChatTab::Stats => "Stats",
        }
    }
}

/// The sections of the quark info panel, selected by a segmented tab bar so the panel
/// stays short instead of one long scroll.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum InfoTab {
    /// Identity (role, state, adoption, Restart action) + personal telemetry, tool usage, and milestones.
    Home,
    /// How it is wired: provider, agent command, model, transport, effort, permission.
    Config,
    /// Its telemetry over the selected [`StatsWindow`].
    Stats,
}

impl InfoTab {
    pub(super) const ALL: [InfoTab; 3] = [InfoTab::Home, InfoTab::Config, InfoTab::Stats];

    pub(super) fn index(self) -> usize {
        match self {
            InfoTab::Home => 0,
            InfoTab::Config => 1,
            InfoTab::Stats => 2,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(InfoTab::Home)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            InfoTab::Home => "Home",
            InfoTab::Config => "Config",
            InfoTab::Stats => "Stats",
        }
    }
}

/// The sub-tabs of the Git rail — kept independent of [`RightRailTab`] so switching
/// between Terminal/File Tree/Git doesn't disturb which Git section was open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GitSubtab {
    Branches,
    Worktrees,
    Changes,
    Graph,
    Diff3Way,
    Remote,
}

impl GitSubtab {
    pub(super) const ALL: [GitSubtab; 6] = [
        GitSubtab::Branches,
        GitSubtab::Worktrees,
        GitSubtab::Changes,
        GitSubtab::Graph,
        GitSubtab::Diff3Way,
        GitSubtab::Remote,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            GitSubtab::Branches => 0,
            GitSubtab::Worktrees => 1,
            GitSubtab::Changes => 2,
            GitSubtab::Graph => 3,
            GitSubtab::Diff3Way => 4,
            GitSubtab::Remote => 5,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(GitSubtab::Branches)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            GitSubtab::Branches => "Branches",
            GitSubtab::Worktrees => "Worktrees",
            GitSubtab::Changes => "Changes",
            GitSubtab::Graph => "Graph",
            GitSubtab::Diff3Way => "3-Way Diff",
            GitSubtab::Remote => "Remote & Issues",
        }
    }
}

/// The sub-views inside [`RightRailTab::Mission`]: implementation plan & wave DAG,
/// live swarm task feed, and shared Nucleus memory index/features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum MissionSubtab {
    #[default]
    Plan,
    Tasks,
    Nucleus,
}

impl MissionSubtab {
    pub(super) const ALL: [MissionSubtab; 3] = [
        MissionSubtab::Plan,
        MissionSubtab::Tasks,
        MissionSubtab::Nucleus,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            MissionSubtab::Plan => 0,
            MissionSubtab::Tasks => 1,
            MissionSubtab::Nucleus => 2,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(MissionSubtab::Plan)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            MissionSubtab::Plan => "Plan & Waves",
            MissionSubtab::Tasks => "Swarm Tasks",
            MissionSubtab::Nucleus => "Nucleus Memory",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RightRailTab {
    Terminal,
    FileTree,
    Git,
    /// Unified Mission view combining the active Implementation Plan, live Swarm Tasks,
    /// and shared Nucleus memory index/feature map.
    Mission,
    /// Interactive 3D Sphere Topology visualizer showing live active and excited quarks.
    Visualizer,
}

impl RightRailTab {
    pub(super) const ALL: [RightRailTab; 5] = [
        RightRailTab::Terminal,
        RightRailTab::FileTree,
        RightRailTab::Git,
        RightRailTab::Mission,
        RightRailTab::Visualizer,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            RightRailTab::Terminal => 0,
            RightRailTab::FileTree => 1,
            RightRailTab::Git => 2,
            RightRailTab::Mission => 3,
            RightRailTab::Visualizer => 4,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(RightRailTab::Terminal)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            RightRailTab::Terminal => "Terminal",
            RightRailTab::FileTree => "File Tree",
            RightRailTab::Git => "Git",
            RightRailTab::Mission => "Mission",
            RightRailTab::Visualizer => "Visualizer",
        }
    }
}

/// The filter tabs for the roster rail: active quarks in this repo vs the entire catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum RosterTab {
    #[default]
    Active,
    All,
}

impl RosterTab {
    pub(super) const ALL: [RosterTab; 2] = [RosterTab::Active, RosterTab::All];

    pub(super) fn index(self) -> usize {
        match self {
            RosterTab::Active => 0,
            RosterTab::All => 1,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(RosterTab::Active)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            RosterTab::Active => "Active",
            RosterTab::All => "All",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roster_tab_indexing_and_defaults() {
        assert_eq!(RosterTab::default(), RosterTab::Active);
        assert_eq!(RosterTab::Active.index(), 0);
        assert_eq!(RosterTab::All.index(), 1);
        assert_eq!(RosterTab::from_index(0), RosterTab::Active);
        assert_eq!(RosterTab::from_index(1), RosterTab::All);
        assert_eq!(RosterTab::from_index(99), RosterTab::Active);
        assert_eq!(RosterTab::Active.label(), "Active");
        assert_eq!(RosterTab::All.label(), "All");
        assert_eq!(RosterTab::ALL.len(), 2);
    }

    #[test]
    fn test_info_tab_indexing_and_defaults() {
        assert_eq!(InfoTab::Home.index(), 0);
        assert_eq!(InfoTab::Config.index(), 1);
        assert_eq!(InfoTab::Stats.index(), 2);
        assert_eq!(InfoTab::from_index(0), InfoTab::Home);
        assert_eq!(InfoTab::from_index(1), InfoTab::Config);
        assert_eq!(InfoTab::from_index(2), InfoTab::Stats);
        assert_eq!(InfoTab::from_index(99), InfoTab::Home);
        assert_eq!(InfoTab::Home.label(), "Home");
        assert_eq!(InfoTab::Config.label(), "Config");
        assert_eq!(InfoTab::Stats.label(), "Stats");
        assert_eq!(InfoTab::ALL.len(), 3);
    }

    #[test]
    fn test_mission_subtab_and_right_rail_consolidation() {
        assert_eq!(RightRailTab::ALL.len(), 5);
        assert_eq!(RightRailTab::from_index(3), RightRailTab::Mission);
        assert_eq!(RightRailTab::from_index(4), RightRailTab::Visualizer);
        assert_eq!(RightRailTab::Mission.label(), "Mission");
        assert_eq!(MissionSubtab::default(), MissionSubtab::Plan);
        assert_eq!(MissionSubtab::from_index(0), MissionSubtab::Plan);
        assert_eq!(MissionSubtab::from_index(1), MissionSubtab::Tasks);
        assert_eq!(MissionSubtab::from_index(2), MissionSubtab::Nucleus);
        assert_eq!(GitSubtab::from_index(2), GitSubtab::Changes);
    }
}
