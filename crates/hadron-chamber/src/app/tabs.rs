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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InfoTab {
    /// Who this quark is: header, role, state, adoption, and the Restart action.
    Identity,
    /// How it is wired: provider, agent command, model, transport, effort, permission.
    Config,
    /// Its telemetry over the selected [`StatsWindow`].
    Stats,
}

impl InfoTab {
    pub(super) const ALL: [InfoTab; 3] = [InfoTab::Identity, InfoTab::Config, InfoTab::Stats];

    pub(super) fn index(self) -> usize {
        match self {
            InfoTab::Identity => 0,
            InfoTab::Config => 1,
            InfoTab::Stats => 2,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(InfoTab::Identity)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            InfoTab::Identity => "Identity",
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
    Graph,
    Delegation,
}

impl GitSubtab {
    pub(super) const ALL: [GitSubtab; 4] = [
        GitSubtab::Branches,
        GitSubtab::Worktrees,
        GitSubtab::Graph,
        GitSubtab::Delegation,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            GitSubtab::Branches => 0,
            GitSubtab::Worktrees => 1,
            GitSubtab::Graph => 2,
            GitSubtab::Delegation => 3,
        }
    }

    pub(super) fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(GitSubtab::Branches)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            GitSubtab::Branches => "Branches",
            GitSubtab::Worktrees => "Worktrees",
            GitSubtab::Graph => "Graph",
            GitSubtab::Delegation => "Delegation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RightRailTab {
    Terminal,
    FileTree,
    Git,
    Changes,
    Plan,
    /// The live swarm task feed — a projection over the field (`ChamberView.tasks`),
    /// beside `Plan` rather than replacing it: `Plan` is the human-authored `.md`,
    /// this is who's working on what right now.
    Tasks,
    /// Interactive 3D Sphere Topology visualizer showing live active and excited quarks.
    Visualizer,
}

impl RightRailTab {
    pub(super) const ALL: [RightRailTab; 7] = [
        RightRailTab::Terminal,
        RightRailTab::FileTree,
        RightRailTab::Git,
        RightRailTab::Changes,
        RightRailTab::Plan,
        RightRailTab::Tasks,
        RightRailTab::Visualizer,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            RightRailTab::Terminal => 0,
            RightRailTab::FileTree => 1,
            RightRailTab::Git => 2,
            RightRailTab::Changes => 3,
            RightRailTab::Plan => 4,
            RightRailTab::Tasks => 5,
            RightRailTab::Visualizer => 6,
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
            RightRailTab::Changes => "Changes",
            RightRailTab::Plan => "Plan",
            RightRailTab::Tasks => "Tasks",
            RightRailTab::Visualizer => "Visualizer",
        }
    }
}
