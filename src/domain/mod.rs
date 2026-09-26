pub mod branch;
pub mod error;
pub mod history;
pub mod log;
pub mod operation;
pub mod repository_context;
pub mod snapshot;
pub mod status;

pub use branch::{
    build_branch_catalog, BranchCatalog, BranchInfo, BranchKind, LocalBranchEntry,
    RemoteBranchGroup,
};
pub use error::{GitError, PreconditionChange, Result};
pub use history::BranchHistory;
pub use log::{CommitSummary, GraphLine};
pub use operation::{
    operation_classification, ConfirmedEffect, ExpectedEffect, MutationRequest, OperationClass,
    OperationClassification, OperationExecution, OperationFailure, OperationFailureKind,
    OperationPlan, OperationPreconditions, OperationReceipt, OperationRef, OperationResetMode,
    OperationState, PlanningSource, VerificationResult, VerificationStatus,
    OPERATION_CLASSIFICATIONS,
};
pub use snapshot::RepoSnapshot;
pub use status::{RepoStatus, StatusEntry};
