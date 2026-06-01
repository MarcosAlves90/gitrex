#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub commit: String,
    pub subject: String,
    pub kind: BranchKind,
}

impl BranchInfo {
    pub fn is_remote(&self) -> bool {
        matches!(self.kind, BranchKind::Remote)
    }

    pub fn remote_name(&self) -> Option<&str> {
        if !self.is_remote() {
            return None;
        }

        self.name.split_once('/').map(|(remote, _)| remote)
    }

    pub fn branch_short_name(&self) -> &str {
        if self.is_remote() {
            self.name
                .split_once('/')
                .map(|(_, branch)| branch)
                .unwrap_or(self.name.as_str())
        } else {
            self.name.as_str()
        }
    }

    pub fn display_name(&self) -> String {
        if self.is_remote() {
            match self.remote_name() {
                Some(remote) => format!("{remote}/{}", self.branch_short_name()),
                None => self.name.clone(),
            }
        } else {
            self.name.clone()
        }
    }

    pub fn full_ref(&self) -> String {
        if self.is_remote() {
            match self.remote_name() {
                Some(remote) => format!("refs/remotes/{remote}/{}", self.branch_short_name()),
                None => self.name.clone(),
            }
        } else {
            format!("refs/heads/{}", self.name)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranchGroup {
    pub remote: String,
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBranchEntry {
    pub branch: BranchInfo,
    pub synced_remotes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCatalog {
    pub remotes: Vec<RemoteBranchGroup>,
    pub locals: Vec<LocalBranchEntry>,
}

pub fn build_branch_catalog(branches: &[BranchInfo]) -> BranchCatalog {
    use std::collections::{BTreeMap, BTreeSet};

    let mut remote_groups: BTreeMap<String, Vec<BranchInfo>> = BTreeMap::new();
    let mut local_branches = Vec::new();
    let mut remote_matches: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for branch in branches.iter().cloned() {
        match branch.kind {
            BranchKind::Remote => {
                if let Some(remote) = branch.remote_name() {
                    remote_groups
                        .entry(remote.to_string())
                        .or_default()
                        .push(branch);
                }
            }
            BranchKind::Local => {
                local_branches.push(branch);
            }
        }
    }

    for group in remote_groups.values() {
        for branch in group {
            remote_matches
                .entry(branch.branch_short_name().to_string())
                .or_default()
                .insert(format!(
                    "{}/{}",
                    branch.remote_name().unwrap_or("remote"),
                    branch.branch_short_name()
                ));
        }
    }

    let mut remotes = remote_groups
        .into_iter()
        .map(|(remote, mut branches)| {
            branches.sort_by(|left, right| {
                right
                    .commit
                    .cmp(&left.commit)
                    .then_with(|| left.branch_short_name().cmp(right.branch_short_name()))
            });
            RemoteBranchGroup { remote, branches }
        })
        .collect::<Vec<_>>();

    remotes.sort_by(|left, right| left.remote.cmp(&right.remote));

    local_branches.sort_by(|left, right| {
        right
            .current
            .cmp(&left.current)
            .then_with(|| left.name.cmp(&right.name))
    });

    let locals = local_branches
        .into_iter()
        .map(|branch| {
            let mut synced_remotes = remote_matches
                .get(branch.name.as_str())
                .map(|remotes| remotes.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            if let Some(upstream) = branch.upstream.as_deref() {
                synced_remotes.push(upstream.to_string());
            }
            synced_remotes.sort();
            synced_remotes.dedup();
            LocalBranchEntry {
                branch,
                synced_remotes,
            }
        })
        .collect::<Vec<_>>();

    BranchCatalog { remotes, locals }
}

#[cfg(test)]
mod tests {
    use super::{build_branch_catalog, BranchInfo, BranchKind};

    #[test]
    fn branch_helpers_keep_remote_and_local_names_distinct() {
        let remote = BranchInfo {
            name: "origin/feature/login".to_string(),
            current: false,
            upstream: None,
            commit: "abc123".to_string(),
            subject: "Add login".to_string(),
            kind: BranchKind::Remote,
        };
        let local = BranchInfo {
            name: "feature/login".to_string(),
            current: false,
            upstream: Some("origin/feature/login".to_string()),
            commit: "def456".to_string(),
            subject: "Work in progress".to_string(),
            kind: BranchKind::Local,
        };

        assert_eq!(remote.remote_name(), Some("origin"));
        assert_eq!(remote.branch_short_name(), "feature/login");
        assert_eq!(remote.display_name(), "origin/feature/login");
        assert_eq!(local.remote_name(), None);
        assert_eq!(local.branch_short_name(), "feature/login");
        assert_eq!(local.display_name(), "feature/login");
    }

    #[test]
    fn branch_catalog_groups_remote_refs_and_tracks_synced_locals() {
        let branches = vec![
            BranchInfo {
                name: "main".to_string(),
                current: true,
                upstream: Some("origin/main".to_string()),
                commit: "def456".to_string(),
                subject: "Main commit".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "abc123".to_string(),
                subject: "Feature commit".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "docs".to_string(),
                current: false,
                upstream: None,
                commit: "aaa111".to_string(),
                subject: "Docs commit".to_string(),
                kind: BranchKind::Local,
            },
            BranchInfo {
                name: "origin/main".to_string(),
                current: false,
                upstream: None,
                commit: "def456".to_string(),
                subject: "Main commit".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "upstream/main".to_string(),
                current: false,
                upstream: None,
                commit: "def456".to_string(),
                subject: "Main commit".to_string(),
                kind: BranchKind::Remote,
            },
            BranchInfo {
                name: "origin/feature/login".to_string(),
                current: false,
                upstream: None,
                commit: "abc123".to_string(),
                subject: "Feature commit".to_string(),
                kind: BranchKind::Remote,
            },
        ];

        let catalog = build_branch_catalog(&branches);

        assert_eq!(catalog.remotes.len(), 2);
        assert_eq!(catalog.locals.len(), 3);
        assert_eq!(catalog.locals[0].branch.name, "main");
        assert!(catalog.locals[0]
            .synced_remotes
            .contains(&"origin/main".to_string()));
        assert!(catalog.locals[0]
            .synced_remotes
            .contains(&"upstream/main".to_string()));
        assert!(catalog.locals[1].synced_remotes.is_empty());
        assert!(!catalog.locals[2].synced_remotes.is_empty());
        assert_eq!(catalog.remotes[0].remote, "origin");
        assert_eq!(catalog.remotes[1].remote, "upstream");
    }
}
