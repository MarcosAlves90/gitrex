use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    ReadOnly,
    LocalMutation,
    NetworkRead,
    RemoteMutation,
    Destructive,
}

#[derive(Debug, Clone, Copy)]
pub struct OperationClassification {
    pub name: &'static str,
    pub effects: &'static [OperationClass],
    pub risk_class: OperationClass,
}

const READ: &[OperationClass] = &[OperationClass::ReadOnly];
const LOCAL: &[OperationClass] = &[OperationClass::LocalMutation];
const LOCAL_NETWORK: &[OperationClass] =
    &[OperationClass::LocalMutation, OperationClass::NetworkRead];
const NETWORK_REMOTE: &[OperationClass] =
    &[OperationClass::NetworkRead, OperationClass::RemoteMutation];
const LOCAL_DESTRUCTIVE: &[OperationClass] =
    &[OperationClass::LocalMutation, OperationClass::Destructive];
const NETWORK_DESTRUCTIVE: &[OperationClass] = &[
    OperationClass::NetworkRead,
    OperationClass::RemoteMutation,
    OperationClass::Destructive,
];
const TUI: &[OperationClass] = &[
    OperationClass::ReadOnly,
    OperationClass::LocalMutation,
    OperationClass::NetworkRead,
    OperationClass::RemoteMutation,
    OperationClass::Destructive,
];

pub const OPERATION_CLASSIFICATIONS: &[OperationClassification] = &[
    OperationClassification {
        name: "branch",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "capabilities",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "change-context",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "cherry-pick",
        effects: LOCAL,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "checkout",
        effects: LOCAL,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "checkout-detached",
        effects: LOCAL,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "cleanup",
        effects: LOCAL_DESTRUCTIVE,
        risk_class: OperationClass::Destructive,
    },
    OperationClassification {
        name: "clone",
        effects: LOCAL_NETWORK,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "compare",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "create-branch",
        effects: LOCAL,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "delete-local-branch",
        effects: LOCAL_DESTRUCTIVE,
        risk_class: OperationClass::Destructive,
    },
    OperationClassification {
        name: "delete-remote-branch",
        effects: NETWORK_DESTRUCTIVE,
        risk_class: OperationClass::Destructive,
    },
    OperationClassification {
        name: "diff",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "fetch",
        effects: LOCAL_NETWORK,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "inspect",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "log",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "pull",
        effects: LOCAL_NETWORK,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "push",
        effects: NETWORK_REMOTE,
        risk_class: OperationClass::RemoteMutation,
    },
    OperationClassification {
        name: "reset",
        effects: LOCAL_DESTRUCTIVE,
        risk_class: OperationClass::Destructive,
    },
    OperationClassification {
        name: "show",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "status",
        effects: READ,
        risk_class: OperationClass::ReadOnly,
    },
    OperationClassification {
        name: "switch",
        effects: LOCAL,
        risk_class: OperationClass::LocalMutation,
    },
    OperationClassification {
        name: "tui",
        effects: TUI,
        risk_class: OperationClass::Destructive,
    },
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OperationPreconditions {
    pub expect_head: Option<String>,
    pub expect_branch: Option<String>,
    pub expect_upstream: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OperationState {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub working_tree: Vec<String>,
    pub cherry_pick_head: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationRef {
    pub name: String,
    pub commit_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpectedEffect {
    pub action: String,
    pub target: String,
    pub commit_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningSource {
    GitrexAnalysis,
    GitProvidedDryRun,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationPlan {
    pub operation: String,
    pub effects: Vec<OperationClass>,
    pub risk_class: OperationClass,
    pub preconditions: OperationPreconditions,
    pub precondition_state: OperationState,
    pub observed_state: OperationState,
    pub expected_local_effects: Vec<ExpectedEffect>,
    pub expected_remote_effects: Vec<ExpectedEffect>,
    pub network_access_required: bool,
    pub network_access_during_planning: bool,
    pub planning_source: PlanningSource,
    pub refs: Vec<OperationRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    NotVerified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub checks: Vec<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfirmedEffect {
    pub action: String,
    pub target: String,
    pub before_commit_id: Option<String>,
    pub after_commit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationReceipt {
    pub operation: String,
    pub state_changed: bool,
    pub before: OperationState,
    pub after: OperationState,
    pub confirmed_local_effects: Vec<ConfirmedEffect>,
    pub confirmed_remote_effects: Vec<ConfirmedEffect>,
    pub verification: VerificationResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRequest {
    Checkout {
        target: String,
    },
    CheckoutDetached {
        target: String,
    },
    CherryPick {
        source: String,
        destination: String,
    },
    Reset {
        target: String,
        mode: OperationResetMode,
    },
    Switch {
        target: String,
    },
    CreateBranch {
        name: String,
        from: Option<String>,
    },
    DeleteLocalBranch {
        branch: String,
    },
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    Cleanup {
        base: String,
        branches: Option<Vec<String>>,
        exclusions: Vec<String>,
        remotes: Vec<String>,
    },
    Fetch {
        remote: Option<String>,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        remote: Option<String>,
        branch: Option<String>,
    },
}

impl MutationRequest {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Checkout { .. } => "checkout",
            Self::CheckoutDetached { .. } => "checkout-detached",
            Self::CherryPick { .. } => "cherry-pick",
            Self::Reset { .. } => "reset",
            Self::Switch { .. } => "switch",
            Self::CreateBranch { .. } => "create-branch",
            Self::DeleteLocalBranch { .. } => "delete-local-branch",
            Self::DeleteRemoteBranch { .. } => "delete-remote-branch",
            Self::Cleanup { .. } => "cleanup",
            Self::Fetch { .. } => "fetch",
            Self::Pull { .. } => "pull",
            Self::Push { .. } => "push",
        }
    }

    pub fn identifier(&self) -> &'static str {
        match self {
            Self::Checkout { .. } => "checkout",
            Self::CheckoutDetached { .. } => "checkout_detached",
            Self::CherryPick { .. } => "cherry_pick",
            Self::Reset { .. } => "reset",
            Self::Switch { .. } => "switch",
            Self::CreateBranch { .. } => "create_branch",
            Self::DeleteLocalBranch { .. } => "delete_local_branch",
            Self::DeleteRemoteBranch { .. } => "delete_remote_branch",
            Self::Cleanup { .. } => "cleanup",
            Self::Fetch { .. } => "fetch",
            Self::Pull { .. } => "pull",
            Self::Push { .. } => "push",
        }
    }

    pub fn network_access_required(&self) -> bool {
        matches!(
            self,
            Self::DeleteRemoteBranch { .. }
                | Self::Fetch { .. }
                | Self::Pull { .. }
                | Self::Push { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationFailureKind {
    Execution,
    Verification,
}

#[derive(Debug)]
pub struct OperationFailure {
    pub kind: OperationFailureKind,
    pub error: crate::domain::GitError,
}

#[derive(Debug)]
pub struct OperationExecution {
    pub plan: OperationPlan,
    pub receipt: OperationReceipt,
    pub failure: Option<OperationFailure>,
}

pub fn operation_classification(name: &str) -> Option<&'static OperationClassification> {
    OPERATION_CLASSIFICATIONS
        .iter()
        .find(|classification| classification.name == name)
}
