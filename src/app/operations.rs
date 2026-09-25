use std::collections::{BTreeMap, BTreeSet};

use crate::{
    domain::{
        operation_classification, ConfirmedEffect, ExpectedEffect, GitError, MutationRequest,
        OperationExecution, OperationFailure, OperationFailureKind, OperationPlan,
        OperationPreconditions, OperationReceipt, OperationRef, OperationResetMode, OperationState,
        PlanningSource, PreconditionChange, Result, VerificationResult, VerificationStatus,
    },
    git::{GitClient, ResetMode as GitResetMode, ResetStatus},
};

pub fn plan_mutation(
    client: &GitClient,
    request: &MutationRequest,
    preconditions: &OperationPreconditions,
) -> Result<OperationPlan> {
    let observed_state = read_state(
        client,
        operation_tracks_worktree(request),
        operation_tracks_cherry_pick(request),
    )?;
    check_explicit_preconditions(preconditions, &observed_state)?;

    let classification = operation_classification(request.name())
        .expect("every mutation request must have a registered classification");
    let mut refs = Vec::new();
    add_ref(&mut refs, "HEAD", observed_state.head.clone());
    if let Some(branch) = &observed_state.branch {
        add_ref(
            &mut refs,
            format!("refs/heads/{branch}"),
            read_ref_oid(client, &format!("refs/heads/{branch}"))?,
        );
    }
    if let Some(upstream) = &observed_state.upstream {
        let upstream_ref = format!("refs/remotes/{upstream}");
        add_ref(
            &mut refs,
            upstream_ref.clone(),
            read_ref_oid(client, &upstream_ref)?,
        );
    }

    let mut expected_local_effects = Vec::new();
    let mut expected_remote_effects = Vec::new();

    match request {
        MutationRequest::Checkout { target } => {
            let commit_id = client.resolve_commit(target)?;
            let local_ref = format!("refs/heads/{target}");
            let local_commit = read_ref_oid(client, &local_ref)?;
            if local_commit.is_some() {
                add_ref(&mut refs, local_ref.clone(), local_commit.clone());
                expected_local_effects.push(ExpectedEffect {
                    action: "checkout_to_branch".to_string(),
                    target: local_ref,
                    commit_id: Some(commit_id),
                });
            } else {
                add_ref(&mut refs, target, Some(commit_id.clone()));
                add_revision_ref(client, &mut refs, target)?;
                expected_local_effects.push(ExpectedEffect {
                    action: "checkout_detached".to_string(),
                    target: "HEAD".to_string(),
                    commit_id: Some(commit_id),
                });
            }
        }
        MutationRequest::CheckoutDetached { target } => {
            let commit_id = client.resolve_commit(target)?;
            add_ref(&mut refs, target, Some(commit_id.clone()));
            add_revision_ref(client, &mut refs, target)?;
            expected_local_effects.push(ExpectedEffect {
                action: "checkout_detached".to_string(),
                target: "HEAD".to_string(),
                commit_id: Some(commit_id),
            });
        }
        MutationRequest::CherryPick {
            source,
            destination,
        } => {
            if !observed_state.working_tree.is_empty() {
                return Err(GitError::Backend(
                    "cherry-pick requires a clean index and worktree".into(),
                ));
            }
            let source_id = client.resolve_commit(source)?;
            add_ref(&mut refs, source, Some(source_id.clone()));
            add_revision_ref(client, &mut refs, source)?;
            let destination_ref = format!("refs/heads/{destination}");
            let destination_id = read_ref_oid(client, &destination_ref)?
                .ok_or_else(|| GitError::ReferenceNotFound(destination.clone()))?;
            add_ref(&mut refs, destination_ref.clone(), Some(destination_id));
            expected_local_effects.push(ExpectedEffect {
                action: "cherry_pick_commit".into(),
                target: destination_ref,
                commit_id: Some(source_id),
            });
        }
        MutationRequest::Reset { target, mode } => {
            let branch = observed_state.branch.as_deref().ok_or_else(|| {
                GitError::Backend("reset requires a checked-out local branch".into())
            })?;
            let target_id = client.resolve_commit(target)?;
            add_ref(&mut refs, target, Some(target_id.clone()));
            add_revision_ref(client, &mut refs, target)?;
            let branch_ref = format!("refs/heads/{branch}");
            add_ref(
                &mut refs,
                branch_ref.clone(),
                read_ref_oid(client, &branch_ref)?,
            );
            expected_local_effects.push(ExpectedEffect {
                action: format!("reset_{}", reset_mode_name(*mode)),
                target: branch_ref,
                commit_id: Some(target_id),
            });
        }
        MutationRequest::Switch { target } => {
            let reference = format!("refs/heads/{target}");
            let commit_id = read_ref_oid(client, &reference)?
                .ok_or_else(|| GitError::ReferenceNotFound(target.clone()))?;
            add_ref(&mut refs, reference.clone(), Some(commit_id.clone()));
            expected_local_effects.push(ExpectedEffect {
                action: "switch_to_branch".to_string(),
                target: reference,
                commit_id: Some(commit_id),
            });
        }
        MutationRequest::CreateBranch { name, from } => {
            validate_branch_name(client, name)?;
            let target_ref = format!("refs/heads/{name}");
            if read_ref_oid(client, &target_ref)?.is_some() {
                return Err(GitError::Backend(format!(
                    "local branch '{name}' already exists"
                )));
            }
            let source_name = from.as_deref().unwrap_or("HEAD");
            let commit_id = client.resolve_commit(source_name)?;
            add_ref(&mut refs, source_name, Some(commit_id.clone()));
            add_revision_ref(client, &mut refs, source_name)?;
            add_ref(&mut refs, target_ref.clone(), None);
            expected_local_effects.push(ExpectedEffect {
                action: "create_local_branch".to_string(),
                target: target_ref.clone(),
                commit_id: Some(commit_id.clone()),
            });
            expected_local_effects.push(ExpectedEffect {
                action: "switch_to_branch".to_string(),
                target: target_ref,
                commit_id: Some(commit_id),
            });
        }
        MutationRequest::DeleteLocalBranch { branch } => {
            let reference = format!("refs/heads/{branch}");
            let commit_id = read_ref_oid(client, &reference)?
                .ok_or_else(|| GitError::ReferenceNotFound(branch.clone()))?;
            add_ref(&mut refs, reference.clone(), Some(commit_id));
            expected_local_effects.push(ExpectedEffect {
                action: "delete_local_branch".to_string(),
                target: reference,
                commit_id: None,
            });
        }
        MutationRequest::DeleteRemoteBranch { remote, branch } => {
            expected_remote_effects.push(ExpectedEffect {
                action: "delete_remote_branch".to_string(),
                target: format!("{remote}/refs/heads/{branch}"),
                commit_id: None,
            });
        }
        MutationRequest::Cleanup {
            base,
            branches,
            exclusions,
            remotes,
        } => {
            let base_id = client.resolve_commit(base)?;
            add_ref(&mut refs, base, Some(base_id.clone()));
            add_revision_ref(client, &mut refs, base)?;
            let mut candidates = client.merged_local_branches(base, exclusions, remotes)?;
            if let Some(branches) = branches {
                candidates.retain(|candidate| branches.iter().any(|name| name == &candidate.name));
            }
            for candidate in candidates {
                let reference = format!("refs/heads/{}", candidate.name);
                add_ref(&mut refs, reference.clone(), Some(candidate.commit.clone()));
                expected_local_effects.push(ExpectedEffect {
                    action: "delete_local_branch".to_string(),
                    target: reference,
                    commit_id: None,
                });
            }
        }
        MutationRequest::Fetch { remote } => {
            expected_local_effects.push(ExpectedEffect {
                action: "update_remote_tracking_refs".to_string(),
                target: remote
                    .as_deref()
                    .map(|name| format!("refs/remotes/{name}"))
                    .unwrap_or_else(|| "refs/remotes".to_string()),
                commit_id: None,
            });
            for (name, commit_id) in read_remote_tracking_refs(client, remote.as_deref())? {
                add_ref(&mut refs, name, Some(commit_id));
            }
        }
        MutationRequest::Pull { remote, branch } => {
            if let Some((remote, branch)) =
                pull_target(remote.as_deref(), branch.as_deref(), &observed_state)
            {
                let reference = format!("refs/remotes/{remote}/{branch}");
                let commit_id = read_ref_oid(client, &reference)?;
                add_ref(&mut refs, reference.clone(), commit_id.clone());
                expected_local_effects.push(ExpectedEffect {
                    action: "fast_forward_current_branch".to_string(),
                    target: observed_state
                        .branch
                        .as_deref()
                        .map(|name| format!("refs/heads/{name}"))
                        .unwrap_or_else(|| "HEAD".to_string()),
                    commit_id,
                });
            } else {
                expected_local_effects.push(ExpectedEffect {
                    action: "fast_forward_current_branch".to_string(),
                    target: observed_state
                        .branch
                        .as_deref()
                        .map(|name| format!("refs/heads/{name}"))
                        .unwrap_or_else(|| "HEAD".to_string()),
                    commit_id: None,
                });
            }
        }
        MutationRequest::Push { remote, branch } => {
            if let Some(head) = observed_state.head.clone() {
                add_ref(&mut refs, "HEAD", Some(head.clone()));
                if let Some((remote, branch)) =
                    push_target(remote.as_deref(), branch.as_deref(), &observed_state)
                {
                    expected_remote_effects.push(ExpectedEffect {
                        action: "update_remote_branch".to_string(),
                        target: format!("{remote}/refs/heads/{branch}"),
                        commit_id: Some(head),
                    });
                }
            }
        }
    }

    refs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(OperationPlan {
        operation: request.identifier().to_string(),
        effects: classification.effects.to_vec(),
        risk_class: classification.risk_class,
        preconditions: preconditions.clone(),
        precondition_state: observed_state.clone(),
        observed_state,
        expected_local_effects,
        expected_remote_effects,
        network_access_required: request.network_access_required(),
        network_access_during_planning: false,
        planning_source: PlanningSource::GitrexAnalysis,
        refs,
    })
}

pub fn execute_mutation(
    client: &GitClient,
    request: &MutationRequest,
    preconditions: &OperationPreconditions,
) -> Result<OperationExecution> {
    let plan = plan_mutation(client, request, preconditions)?;
    let tracking_before = match request {
        MutationRequest::Fetch { remote } => {
            Some(read_remote_tracking_refs(client, remote.as_deref())?)
        }
        _ => None,
    };
    let remote_target = execution_remote_target(request, &plan.observed_state);
    let remote_before = remote_target
        .as_ref()
        .and_then(|(remote, reference)| read_remote_ref(client, remote, reference).ok().flatten());

    let tracks_worktree = operation_tracks_worktree(request);
    let tracks_cherry_pick = operation_tracks_cherry_pick(request);
    let before = read_state(client, tracks_worktree, tracks_cherry_pick)?;
    check_explicit_preconditions(preconditions, &before)?;
    if before != plan.observed_state {
        return Err(precondition_error(&plan.observed_state, &before));
    }
    if matches!(request, MutationRequest::CherryPick { .. }) && !before.working_tree.is_empty() {
        return Err(GitError::Backend(
            "cherry-pick requires a clean index and worktree".into(),
        ));
    }
    let local_refs_before = read_relevant_refs(client, &plan)?;
    validate_plan_refs(&plan, &local_refs_before, &before)?;

    let execution_error = perform_mutation(client, request, &plan).err();
    let after_state_result = read_state(client, tracks_worktree, tracks_cherry_pick);
    let (after, after_state_error) = match after_state_result {
        Ok(state) => (state, None),
        Err(error) => (OperationState::default(), Some(error)),
    };
    let local_refs_after_result = read_relevant_refs(client, &plan);
    let tracking_after_result = match request {
        MutationRequest::Fetch { remote } => {
            Some(read_remote_tracking_refs(client, remote.as_deref()))
        }
        _ => None,
    };
    let (refs_after, refs_read_error) = match local_refs_after_result {
        Ok(refs) => (refs, None),
        Err(error) => (
            BTreeMap::new(),
            Some(GitError::VerificationFailed(format!(
                "could not re-read affected refs: {error}"
            ))),
        ),
    };
    let (tracking_after, tracking_read_error) = match tracking_after_result {
        Some(Ok(refs)) => (refs, None),
        Some(Err(error)) => (
            BTreeMap::new(),
            Some(GitError::VerificationFailed(format!(
                "could not re-read remote-tracking refs: {error}"
            ))),
        ),
        None => (BTreeMap::new(), None),
    };
    let (remote_after, remote_read_error, remote_read_succeeded) = match remote_target.as_ref() {
        Some((remote, reference)) => match read_remote_ref(client, remote, reference) {
            Ok(value) => (value, None, true),
            Err(_) => (
                None,
                Some(GitError::VerificationFailed(format!(
                    "could not verify remote ref '{remote}/{reference}' after mutation"
                ))),
                false,
            ),
        },
        None => (None, None, false),
    };
    let after_read_error = after_state_error
        .or(refs_read_error)
        .or(tracking_read_error)
        .or(remote_read_error);

    let (verification, failure) = if let Some(error) = execution_error {
        (
            VerificationResult {
                status: VerificationStatus::NotVerified,
                checks: vec!["post_operation_state_read".to_string()],
                detail: Some("Git execution failed before verification completed".to_string()),
            },
            Some(OperationFailure {
                kind: OperationFailureKind::Execution,
                error,
            }),
        )
    } else if let Some(error) = after_read_error {
        (
            VerificationResult {
                status: VerificationStatus::Failed,
                checks: vec!["post_operation_state_read".to_string()],
                detail: Some(error.to_string()),
            },
            Some(OperationFailure {
                kind: OperationFailureKind::Verification,
                error,
            }),
        )
    } else {
        match verify_mutation(
            client,
            request,
            &plan,
            MutationPostState {
                after: &after,
                refs_after: &refs_after,
                tracking_after: &tracking_after,
                remote_target: remote_target.as_ref(),
                remote_after: remote_after.as_deref(),
            },
        ) {
            Ok(checks) => (
                VerificationResult {
                    status: VerificationStatus::Verified,
                    checks,
                    detail: None,
                },
                None,
            ),
            Err(error) => (
                VerificationResult {
                    status: VerificationStatus::Failed,
                    checks: vec!["operation_specific_postcondition".to_string()],
                    detail: Some(error.to_string()),
                },
                Some(OperationFailure {
                    kind: OperationFailureKind::Verification,
                    error,
                }),
            ),
        }
    };

    let confirmed_local_effects = confirmed_local_effects(
        request,
        &plan,
        LocalEffectObservation {
            before: &before,
            after: &after,
            refs_before: &local_refs_before,
            refs_after: &refs_after,
            tracking_before: tracking_before.as_ref(),
            tracking_after: &tracking_after,
            verified: verification.status == VerificationStatus::Verified,
        },
    );
    let confirmed_remote_effects = confirmed_remote_effects(
        request,
        &plan,
        RemoteEffectObservation {
            target: remote_target.as_ref(),
            before: remote_before.clone(),
            after: remote_after.clone(),
            after_was_read: remote_read_succeeded,
        },
    );
    let state_changed = before != after
        || local_refs_before != refs_after
        || tracking_before
            .as_ref()
            .is_some_and(|before| before != &tracking_after)
        || remote_target.is_some() && remote_before != remote_after;

    Ok(OperationExecution {
        plan,
        receipt: OperationReceipt {
            operation: request.identifier().to_string(),
            state_changed,
            before,
            after,
            confirmed_local_effects,
            confirmed_remote_effects,
            verification,
        },
        failure,
    })
}

fn perform_mutation(
    client: &GitClient,
    request: &MutationRequest,
    plan: &OperationPlan,
) -> Result<()> {
    match request {
        MutationRequest::Checkout { target } => client.checkout(target),
        MutationRequest::CheckoutDetached { .. } => {
            let commit_id = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::Backend("planned detached checkout commit is missing".into())
                })?;
            client.git().run(["switch", "--detach", "--", commit_id])?;
            Ok(())
        }
        MutationRequest::CherryPick {
            source: _,
            destination,
        } => {
            let source_id = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::Backend("planned cherry-pick source commit is missing".into())
                })?;
            let result = client.cherry_pick_planned_to_branch(source_id, destination)?;
            match result.status {
                crate::git::CherryPickStatus::Applied => Ok(()),
                crate::git::CherryPickStatus::Conflict => Err(GitError::Backend(format!(
                    "cherry-pick conflict: {}",
                    result.detail
                ))),
                crate::git::CherryPickStatus::Stopped => Err(GitError::Backend(format!(
                    "cherry-pick stopped: {}",
                    result.detail
                ))),
                crate::git::CherryPickStatus::Failed => Err(GitError::Backend(format!(
                    "cherry-pick failed: {}",
                    result.detail
                ))),
            }
        }
        MutationRequest::Reset { mode, .. } => {
            let target_id = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::Backend("planned reset target commit is missing".into())
                })?;
            let branch = plan.observed_state.branch.as_deref().ok_or_else(|| {
                GitError::Backend("reset requires a checked-out local branch".into())
            })?;
            let head = plan.observed_state.head.as_deref().ok_or_else(|| {
                GitError::Backend("reset requires an available HEAD commit".into())
            })?;
            let result = client.reset_to_commit(target_id, git_reset_mode(*mode), branch, head)?;
            if result.status == ResetStatus::Applied {
                Ok(())
            } else {
                Err(GitError::Backend(result.detail))
            }
        }
        MutationRequest::Switch { target } => client.switch(target),
        MutationRequest::CreateBranch { name, .. } => {
            let source = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::Backend("planned branch start commit is missing".into())
                })?;
            client.create_branch(name, Some(source))
        }
        MutationRequest::DeleteLocalBranch { branch } => client.delete_local_branch(branch),
        MutationRequest::DeleteRemoteBranch { remote, branch } => {
            client.delete_remote_branch(remote, branch)
        }
        MutationRequest::Cleanup {
            base,
            exclusions,
            remotes,
            ..
        } => {
            let mut failures = Vec::new();
            let base_oid = plan
                .refs
                .iter()
                .find(|reference| reference.name == *base)
                .and_then(|reference| reference.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::Backend("planned cleanup base commit is missing".into())
                })?;
            let eligible = if remotes.is_empty() {
                None
            } else {
                Some(
                    client
                        .merged_local_branches(base_oid, exclusions, remotes)?
                        .into_iter()
                        .map(|candidate| candidate.name)
                        .collect::<BTreeSet<_>>(),
                )
            };
            for effect in &plan.expected_local_effects {
                if effect.action != "delete_local_branch" {
                    continue;
                }
                let branch = effect.target.strip_prefix("refs/heads/").ok_or_else(|| {
                    GitError::Backend("planned cleanup branch reference is invalid".into())
                })?;
                if eligible
                    .as_ref()
                    .is_some_and(|eligible| !eligible.contains(branch))
                {
                    continue;
                }
                if let Err(error) = client.delete_local_branch(branch) {
                    failures.push(format!("{branch}: {error}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(GitError::Backend(format!(
                    "cleanup failed for {}",
                    failures.join("; ")
                )))
            }
        }
        MutationRequest::Fetch { remote } => client.fetch(remote.as_deref()),
        MutationRequest::Pull { remote, branch } => {
            client.pull(remote.as_deref(), branch.as_deref())
        }
        MutationRequest::Push { remote, branch } => {
            client.push(remote.as_deref(), branch.as_deref())
        }
    }
}

struct MutationPostState<'a> {
    after: &'a OperationState,
    refs_after: &'a BTreeMap<String, Option<String>>,
    tracking_after: &'a BTreeMap<String, String>,
    remote_target: Option<&'a (String, String)>,
    remote_after: Option<&'a str>,
}

fn verify_mutation(
    client: &GitClient,
    request: &MutationRequest,
    plan: &OperationPlan,
    post: MutationPostState<'_>,
) -> Result<Vec<String>> {
    let MutationPostState {
        after,
        refs_after,
        tracking_after,
        remote_target,
        remote_after,
    } = post;
    let mut checks = Vec::new();
    match request {
        MutationRequest::CreateBranch { name, .. } => {
            let target_ref = format!("refs/heads/{name}");
            let expected = plan.expected_local_effects[0].commit_id.as_deref();
            if after.branch.as_deref() != Some(name)
                || refs_after.get(&target_ref).and_then(Option::as_deref) != expected
            {
                return Err(GitError::VerificationFailed(format!(
                    "branch '{name}' was not checked out at its planned commit"
                )));
            }
            checks.push("created_branch_matches_planned_commit".to_string());
        }
        MutationRequest::Switch { target } => {
            let expected = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref());
            if after.branch.as_deref() != Some(target) || after.head.as_deref() != expected {
                return Err(GitError::VerificationFailed(format!(
                    "current branch '{target}' is not at its planned commit after switch"
                )));
            }
            checks.push("current_branch_matches_target".to_string());
        }
        MutationRequest::Checkout { target } | MutationRequest::CheckoutDetached { target } => {
            let expected = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref());
            if after.head.as_deref() != expected {
                return Err(GitError::VerificationFailed(format!(
                    "HEAD does not match the planned commit for '{target}'"
                )));
            }
            if let Some(local_effect) = plan.expected_local_effects.first() {
                if local_effect.action == "checkout_to_branch"
                    && after.branch.as_deref() != Some(target.as_str())
                {
                    return Err(GitError::VerificationFailed(format!(
                        "current branch is not '{target}' after checkout"
                    )));
                }
                if local_effect.action == "checkout_detached" && after.branch.is_some() {
                    return Err(GitError::VerificationFailed(
                        "HEAD is not detached after checkout".to_string(),
                    ));
                }
            }
            checks.push("head_matches_checkout_target".to_string());
        }
        MutationRequest::CherryPick { destination, .. } => {
            let destination_ref = format!("refs/heads/{destination}");
            let before_commit = plan
                .refs
                .iter()
                .find(|reference| reference.name == destination_ref)
                .and_then(|reference| reference.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::VerificationFailed(
                        "planned cherry-pick destination commit is missing".into(),
                    )
                })?;
            let head = after.head.as_deref().ok_or_else(|| {
                GitError::VerificationFailed("HEAD is unavailable after cherry-pick".into())
            })?;
            let parents =
                probe_text(client, &["show", "-s", "--format=%P", head])?.unwrap_or_default();
            if after.branch.as_deref() != Some(destination.as_str())
                || head == before_commit
                || refs_after.get(&destination_ref).and_then(Option::as_deref) != Some(head)
                || parents.split_whitespace().next() != Some(before_commit)
                || after.cherry_pick_head.is_some()
            {
                return Err(GitError::VerificationFailed(format!(
                    "cherry-pick did not create a commit on branch '{destination}' from its planned tip"
                )));
            }
            checks.push("cherry_pick_commit_has_planned_destination_parent".to_string());
        }
        MutationRequest::Reset { mode, .. } => {
            let target = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::VerificationFailed("planned reset target commit is missing".into())
                })?;
            let branch = plan.observed_state.branch.as_deref().ok_or_else(|| {
                GitError::VerificationFailed("planned reset branch is missing".into())
            })?;
            let branch_ref = format!("refs/heads/{branch}");
            if after.branch.as_deref() != Some(branch)
                || after.head.as_deref() != Some(target)
                || refs_after.get(&branch_ref).and_then(Option::as_deref) != Some(target)
            {
                return Err(GitError::VerificationFailed(format!(
                    "reset did not move branch '{branch}' to its planned commit"
                )));
            }
            match mode {
                OperationResetMode::Mixed | OperationResetMode::Hard => {
                    let index_matches = client.git().probe(["diff", "--cached", "--quiet"])?;
                    if !index_matches.success() {
                        return Err(GitError::VerificationFailed(
                            "reset left staged changes after moving the index".into(),
                        ));
                    }
                    checks.push("index_matches_reset_target".to_string());
                }
                OperationResetMode::Soft => {}
            }
            if *mode == OperationResetMode::Hard {
                let worktree_matches = client.git().probe(["diff", "--quiet"])?;
                if !worktree_matches.success() {
                    return Err(GitError::VerificationFailed(
                        "hard reset left tracked worktree changes".into(),
                    ));
                }
                checks.push("tracked_worktree_matches_reset_target".to_string());
            }
            checks.push("branch_matches_reset_target".to_string());
        }
        MutationRequest::DeleteLocalBranch { branch } => {
            if refs_after
                .get(&format!("refs/heads/{branch}"))
                .is_some_and(Option::is_some)
            {
                return Err(GitError::VerificationFailed(format!(
                    "local branch '{branch}' still exists"
                )));
            }
            checks.push("local_branch_absent".to_string());
        }
        MutationRequest::DeleteRemoteBranch { remote, branch } => {
            if remote_after.is_some() {
                return Err(GitError::VerificationFailed(format!(
                    "remote branch '{remote}/{branch}' still exists"
                )));
            }
            checks.push("remote_branch_absent".to_string());
        }
        MutationRequest::Cleanup { .. } => {
            let remaining = plan
                .expected_local_effects
                .iter()
                .filter(|effect| effect.action == "delete_local_branch")
                .filter_map(|effect| effect.target.strip_prefix("refs/heads/"))
                .filter(|branch| {
                    refs_after
                        .get(&format!("refs/heads/{branch}"))
                        .is_some_and(Option::is_some)
                })
                .collect::<Vec<_>>();
            if !remaining.is_empty() {
                return Err(GitError::VerificationFailed(format!(
                    "planned cleanup branches remain: {}",
                    remaining.join(", ")
                )));
            }
            checks.push("planned_cleanup_branches_absent".to_string());
        }
        MutationRequest::Fetch { remote } => {
            verify_fetch_refs(client, remote.as_deref(), tracking_after)?;
            checks.push("remote_tracking_refs_match_advertised_remote_refs".to_string());
        }
        MutationRequest::Pull { .. } => {
            let fetched_commit = read_ref_oid(client, "FETCH_HEAD")?.ok_or_else(|| {
                GitError::VerificationFailed("FETCH_HEAD was not written by pull".to_string())
            })?;
            let head = after.head.as_deref().ok_or_else(|| {
                GitError::VerificationFailed("HEAD is unavailable after pull".to_string())
            })?;
            let output = client.git().probe([
                "merge-base",
                "--is-ancestor",
                fetched_commit.as_str(),
                head,
            ])?;
            if !output.success() {
                return Err(GitError::VerificationFailed(
                    "pulled commit is not contained in the resulting HEAD".to_string(),
                ));
            }
            checks.push("fetched_commit_is_ancestor_of_head".to_string());
        }
        MutationRequest::Push { .. } => {
            let (remote, reference) = remote_target.ok_or_else(|| {
                GitError::VerificationFailed(
                    "could not determine the remote branch affected by push".to_string(),
                )
            })?;
            let expected = plan
                .expected_remote_effects
                .first()
                .and_then(|effect| effect.commit_id.as_deref())
                .ok_or_else(|| {
                    GitError::VerificationFailed("planned push commit is unavailable".into())
                })?;
            if remote_after != Some(expected) {
                return Err(GitError::VerificationFailed(format!(
                    "remote branch '{remote}/{reference}' does not point to the pushed commit"
                )));
            }
            checks.push("remote_branch_matches_pushed_commit".to_string());
        }
    }
    Ok(checks)
}

struct LocalEffectObservation<'a> {
    before: &'a OperationState,
    after: &'a OperationState,
    refs_before: &'a BTreeMap<String, Option<String>>,
    refs_after: &'a BTreeMap<String, Option<String>>,
    tracking_before: Option<&'a BTreeMap<String, String>>,
    tracking_after: &'a BTreeMap<String, String>,
    verified: bool,
}

fn confirmed_local_effects(
    request: &MutationRequest,
    plan: &OperationPlan,
    observation: LocalEffectObservation<'_>,
) -> Vec<ConfirmedEffect> {
    let LocalEffectObservation {
        before,
        after,
        refs_before,
        refs_after,
        tracking_before,
        tracking_after,
        verified,
    } = observation;
    match request {
        MutationRequest::CreateBranch { name, .. } => {
            let reference = format!("refs/heads/{name}");
            let after_commit = refs_after.get(&reference).cloned().flatten();
            let expected = plan.expected_local_effects[0].commit_id.clone();
            if after.branch.as_deref() == Some(name) && after_commit == expected {
                vec![ConfirmedEffect {
                    action: "create_and_checkout_branch".to_string(),
                    target: reference,
                    before_commit_id: refs_before
                        .get(&format!("refs/heads/{name}"))
                        .cloned()
                        .flatten(),
                    after_commit_id: after_commit,
                }]
            } else {
                Vec::new()
            }
        }
        MutationRequest::Switch { target }
        | MutationRequest::Checkout { target }
        | MutationRequest::CheckoutDetached { target } => {
            if !verified {
                return Vec::new();
            }
            let expected = plan
                .expected_local_effects
                .first()
                .and_then(|effect| effect.commit_id.clone());
            vec![ConfirmedEffect {
                action: "set_head".to_string(),
                target: target.clone(),
                before_commit_id: before.head.clone(),
                after_commit_id: expected,
            }]
        }
        MutationRequest::CherryPick { destination, .. } if verified => {
            let reference = format!("refs/heads/{destination}");
            vec![ConfirmedEffect {
                action: "cherry_pick_commit".into(),
                target: reference.clone(),
                before_commit_id: refs_before.get(&reference).cloned().flatten(),
                after_commit_id: refs_after.get(&reference).cloned().flatten(),
            }]
        }
        MutationRequest::Reset { .. } if verified => {
            let Some(branch) = plan.observed_state.branch.as_deref() else {
                return Vec::new();
            };
            let reference = format!("refs/heads/{branch}");
            vec![ConfirmedEffect {
                action: "reset_branch".into(),
                target: reference.clone(),
                before_commit_id: refs_before.get(&reference).cloned().flatten(),
                after_commit_id: refs_after.get(&reference).cloned().flatten(),
            }]
        }
        MutationRequest::CherryPick { .. } | MutationRequest::Reset { .. } => Vec::new(),
        MutationRequest::DeleteLocalBranch { branch } => {
            let reference = format!("refs/heads/{branch}");
            if refs_before.get(&reference).is_some_and(Option::is_some)
                && refs_after.get(&reference).is_some_and(Option::is_none)
            {
                vec![ConfirmedEffect {
                    action: "delete_local_branch".to_string(),
                    target: reference.clone(),
                    before_commit_id: refs_before.get(&reference).cloned().flatten(),
                    after_commit_id: None,
                }]
            } else {
                Vec::new()
            }
        }
        MutationRequest::Cleanup { .. } => plan
            .expected_local_effects
            .iter()
            .filter(|effect| effect.action == "delete_local_branch")
            .filter(|effect| {
                refs_before.get(&effect.target).is_some_and(Option::is_some)
                    && refs_after.get(&effect.target).is_some_and(Option::is_none)
            })
            .map(|effect| ConfirmedEffect {
                action: "delete_local_branch".to_string(),
                target: effect.target.clone(),
                before_commit_id: refs_before.get(&effect.target).cloned().flatten(),
                after_commit_id: None,
            })
            .collect(),
        MutationRequest::Fetch { .. } => tracking_before
            .into_iter()
            .flat_map(|before| {
                before
                    .keys()
                    .chain(tracking_after.keys())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .filter_map(|reference| {
                let before = tracking_before.and_then(|map| map.get(reference)).cloned();
                let after = tracking_after.get(reference).cloned();
                (before != after).then(|| ConfirmedEffect {
                    action: "update_remote_tracking_ref".to_string(),
                    target: reference.clone(),
                    before_commit_id: before,
                    after_commit_id: after,
                })
            })
            .collect(),
        MutationRequest::Pull { .. } if verified => vec![ConfirmedEffect {
            action: "fast_forward_current_branch".to_string(),
            target: after
                .branch
                .as_deref()
                .map(|branch| format!("refs/heads/{branch}"))
                .unwrap_or_else(|| "HEAD".to_string()),
            before_commit_id: before.head.clone(),
            after_commit_id: after.head.clone(),
        }],
        MutationRequest::Push { .. } | MutationRequest::DeleteRemoteBranch { .. } => Vec::new(),
        MutationRequest::Pull { .. } => Vec::new(),
    }
}

struct RemoteEffectObservation<'a> {
    target: Option<&'a (String, String)>,
    before: Option<String>,
    after: Option<String>,
    after_was_read: bool,
}

fn confirmed_remote_effects(
    request: &MutationRequest,
    plan: &OperationPlan,
    observation: RemoteEffectObservation<'_>,
) -> Vec<ConfirmedEffect> {
    let RemoteEffectObservation {
        target,
        before,
        after,
        after_was_read,
    } = observation;
    if !after_was_read {
        return Vec::new();
    }
    match request {
        MutationRequest::Push { .. } => {
            let expected = plan
                .expected_remote_effects
                .first()
                .and_then(|effect| effect.commit_id.clone());
            match (target, expected, after) {
                (Some((remote, reference)), Some(expected), Some(after)) if after == expected => {
                    vec![ConfirmedEffect {
                        action: "update_remote_branch".to_string(),
                        target: format!("{remote}/{reference}"),
                        before_commit_id: before,
                        after_commit_id: Some(after),
                    }]
                }
                _ => Vec::new(),
            }
        }
        MutationRequest::DeleteRemoteBranch { remote, branch }
            if after.is_none() && before.is_some() =>
        {
            vec![ConfirmedEffect {
                action: "delete_remote_branch".to_string(),
                target: format!("{remote}/refs/heads/{branch}"),
                before_commit_id: before,
                after_commit_id: None,
            }]
        }
        _ => Vec::new(),
    }
}

fn check_explicit_preconditions(
    expected: &OperationPreconditions,
    observed: &OperationState,
) -> Result<()> {
    let head_matches = expected.expect_head.as_ref().is_none_or(|head| {
        observed
            .head
            .as_ref()
            .is_some_and(|observed_head| observed_head.eq_ignore_ascii_case(head))
    });
    let branch_matches = expected
        .expect_branch
        .as_ref()
        .is_none_or(|branch| observed.branch.as_ref() == Some(branch));
    let upstream_matches = expected
        .expect_upstream
        .as_ref()
        .is_none_or(|upstream| observed.upstream.as_ref() == Some(upstream));
    if head_matches && branch_matches && upstream_matches {
        Ok(())
    } else {
        Err(GitError::PreconditionChanged {
            details: Box::new(PreconditionChange {
                expected_head: expected.expect_head.clone(),
                expected_branch: expected.expect_branch.clone(),
                expected_upstream: expected.expect_upstream.clone(),
                observed_head: observed.head.clone(),
                observed_branch: observed.branch.clone(),
                observed_upstream: observed.upstream.clone(),
                changed_reference: None,
                expected_reference_commit_id: None,
                observed_reference_commit_id: None,
            }),
        })
    }
}

fn precondition_error(expected: &OperationState, observed: &OperationState) -> GitError {
    GitError::PreconditionChanged {
        details: Box::new(PreconditionChange {
            expected_head: expected.head.clone(),
            expected_branch: expected.branch.clone(),
            expected_upstream: expected.upstream.clone(),
            observed_head: observed.head.clone(),
            observed_branch: observed.branch.clone(),
            observed_upstream: observed.upstream.clone(),
            changed_reference: None,
            expected_reference_commit_id: None,
            observed_reference_commit_id: None,
        }),
    }
}

fn validate_plan_refs(
    plan: &OperationPlan,
    observed_refs: &BTreeMap<String, Option<String>>,
    observed_state: &OperationState,
) -> Result<()> {
    for reference in &plan.refs {
        if !reference.name.starts_with("refs/") && reference.name != "HEAD" {
            continue;
        }
        let observed = observed_refs
            .get(&reference.name)
            .cloned()
            .unwrap_or_default();
        if observed != reference.commit_id {
            return Err(GitError::PreconditionChanged {
                details: Box::new(PreconditionChange {
                    expected_head: plan.observed_state.head.clone(),
                    expected_branch: plan.observed_state.branch.clone(),
                    expected_upstream: plan.observed_state.upstream.clone(),
                    observed_head: observed_state.head.clone(),
                    observed_branch: observed_state.branch.clone(),
                    observed_upstream: observed_state.upstream.clone(),
                    changed_reference: Some(reference.name.clone()),
                    expected_reference_commit_id: reference.commit_id.clone(),
                    observed_reference_commit_id: observed,
                }),
            });
        }
    }
    Ok(())
}

fn read_state(
    client: &GitClient,
    include_worktree: bool,
    include_cherry_pick_head: bool,
) -> Result<OperationState> {
    let git = client.git();
    git.ensure_repository()?;
    let (head, branch, upstream, working_tree) = if include_worktree {
        let status = git.run_text([
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=all",
        ])?;
        let mut head = None;
        let mut branch = None;
        let mut upstream = None;
        let mut working_tree = Vec::new();
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("# branch.oid ") {
                head = (!matches!(value, "(initial)" | "(unknown)")).then(|| value.to_owned());
            } else if let Some(value) = line.strip_prefix("# branch.head ") {
                branch = (value != "(detached)").then(|| value.to_owned());
            } else if let Some(value) = line.strip_prefix("# branch.upstream ") {
                upstream = Some(value.to_owned());
            } else if !line.starts_with('#') && !line.is_empty() {
                working_tree.push(line.to_owned());
            }
        }
        (head, branch, upstream, working_tree)
    } else {
        (
            probe_text(
                client,
                &["rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"],
            )?,
            probe_text(client, &["symbolic-ref", "--quiet", "--short", "HEAD"])?,
            probe_text(
                client,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "--verify",
                    "--end-of-options",
                    "@{upstream}",
                ],
            )?,
            Vec::new(),
        )
    };
    Ok(OperationState {
        head,
        branch,
        upstream,
        working_tree,
        cherry_pick_head: if include_cherry_pick_head {
            probe_text(
                client,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    "CHERRY_PICK_HEAD",
                ],
            )?
        } else {
            None
        },
    })
}

fn probe_text(client: &GitClient, args: &[&str]) -> Result<Option<String>> {
    let output = client.git().probe(args)?;
    if !output.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
    let value = text.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn read_ref_oid(client: &GitClient, reference: &str) -> Result<Option<String>> {
    probe_text(
        client,
        &["rev-parse", "--verify", "--end-of-options", reference],
    )
}

fn validate_branch_name(client: &GitClient, name: &str) -> Result<()> {
    let output = client.git().probe(["check-ref-format", "--branch", name])?;
    if output.success() {
        Ok(())
    } else {
        Err(GitError::Backend(format!("invalid branch name '{name}'")))
    }
}

fn add_ref(refs: &mut Vec<OperationRef>, name: impl Into<String>, commit_id: Option<String>) {
    let name = name.into();
    if let Some(existing) = refs.iter_mut().find(|reference| reference.name == name) {
        existing.commit_id = commit_id;
    } else {
        refs.push(OperationRef { name, commit_id });
    }
}

fn add_revision_ref(
    client: &GitClient,
    refs: &mut Vec<OperationRef>,
    revision: &str,
) -> Result<()> {
    if revision.contains(['^', '~', ':']) {
        return Ok(());
    }
    let Some(reference) = probe_text(
        client,
        &[
            "rev-parse",
            "--symbolic-full-name",
            "--verify",
            "--end-of-options",
            revision,
        ],
    )?
    else {
        return Ok(());
    };
    if reference.starts_with("refs/") {
        let commit_id = read_ref_oid(client, &reference)?;
        add_ref(refs, reference, commit_id);
    }
    Ok(())
}

fn read_relevant_refs(
    client: &GitClient,
    plan: &OperationPlan,
) -> Result<BTreeMap<String, Option<String>>> {
    let output = client
        .git()
        .run_text(["for-each-ref", "--format=%(refname)%09%(objectname)"])?;
    let all_refs = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, commit_id)| (name.to_owned(), commit_id.to_owned()))
        .collect::<BTreeMap<_, _>>();
    let mut refs = BTreeMap::new();
    let head = read_ref_oid(client, "HEAD")?;
    for reference in &plan.refs {
        if reference.name == "HEAD" {
            refs.insert(reference.name.clone(), head.clone());
        } else if reference.name.starts_with("refs/") {
            refs.insert(
                reference.name.clone(),
                all_refs.get(&reference.name).cloned(),
            );
        }
    }
    Ok(refs)
}

fn read_remote_tracking_refs(
    client: &GitClient,
    remote: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    let mut args = vec![
        "for-each-ref".to_string(),
        "--format=%(refname)%09%(objectname)".to_string(),
    ];
    let pattern = remote.map(|name| format!("refs/remotes/{name}"));
    if let Some(pattern) = &pattern {
        args.push("--".to_string());
        args.push(pattern.clone());
    }
    let output = client.git().run_text(args.iter().map(String::as_str))?;
    let mut refs = BTreeMap::new();
    for line in output.lines() {
        let Some((name, commit_id)) = line.split_once('\t') else {
            continue;
        };
        if name.starts_with("refs/remotes/") && !commit_id.is_empty() {
            refs.insert(name.to_string(), commit_id.to_string());
        }
    }
    Ok(refs)
}

fn advertised_remote_heads(client: &GitClient, remote: &str) -> Result<BTreeMap<String, String>> {
    let output = client
        .git()
        .run_text(["ls-remote", "--heads", "--", remote])?;
    let mut refs = BTreeMap::new();
    for line in output.lines() {
        let Some((commit_id, reference)) = line.split_once('\t') else {
            continue;
        };
        refs.insert(reference.to_string(), commit_id.to_string());
    }
    Ok(refs)
}

fn verify_fetch_refs(
    client: &GitClient,
    remote: Option<&str>,
    tracking_after: &BTreeMap<String, String>,
) -> Result<()> {
    let remotes = if let Some(remote) = remote {
        vec![remote.to_string()]
    } else {
        client
            .git()
            .run_text(["remote"])?
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    };

    for remote in remotes {
        let advertised = advertised_remote_heads(client, &remote).map_err(|_| {
            GitError::VerificationFailed(format!(
                "could not verify fetched refs for remote '{remote}'"
            ))
        })?;
        for (remote_ref, commit_id) in advertised {
            let Some(branch) = remote_ref.strip_prefix("refs/heads/") else {
                continue;
            };
            let tracking_ref = format!("refs/remotes/{remote}/{branch}");
            if tracking_after.get(&tracking_ref) != Some(&commit_id) {
                return Err(GitError::VerificationFailed(format!(
                    "tracking ref '{tracking_ref}' does not match the advertised remote commit"
                )));
            }
        }
    }
    Ok(())
}

fn read_remote_ref(client: &GitClient, remote: &str, reference: &str) -> Result<Option<String>> {
    let output = client
        .git()
        .probe(["ls-remote", "--heads", "--", remote, reference])?;
    if !output.success() {
        return Err(GitError::Backend(format!(
            "could not read remote ref '{remote}/{reference}'"
        )));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
    Ok(text.lines().find_map(|line| {
        line.split_once('\t')
            .map(|(commit_id, _)| commit_id.to_string())
    }))
}

fn pull_target(
    remote: Option<&str>,
    branch: Option<&str>,
    state: &OperationState,
) -> Option<(String, String)> {
    if let Some(branch) = branch {
        return Some((remote.unwrap_or("origin").to_string(), branch.to_string()));
    }
    if let Some(upstream) = &state.upstream {
        let (upstream_remote, upstream_branch) = upstream.split_once('/')?;
        return Some((
            remote.unwrap_or(upstream_remote).to_string(),
            upstream_branch.to_string(),
        ));
    }
    remote.map(str::to_owned).zip(state.branch.clone())
}

fn push_target(
    remote: Option<&str>,
    branch: Option<&str>,
    state: &OperationState,
) -> Option<(String, String)> {
    if let Some(branch) = branch {
        return Some((remote.unwrap_or("origin").to_string(), branch.to_string()));
    }
    if let Some(upstream) = &state.upstream {
        let (upstream_remote, upstream_branch) = upstream.split_once('/')?;
        return Some((
            remote.unwrap_or(upstream_remote).to_string(),
            upstream_branch.to_string(),
        ));
    }
    remote.map(str::to_owned).zip(state.branch.clone())
}

fn execution_remote_target(
    request: &MutationRequest,
    state: &OperationState,
) -> Option<(String, String)> {
    match request {
        MutationRequest::Push { remote, branch } => {
            push_target(remote.as_deref(), branch.as_deref(), state)
                .map(|(remote, branch)| (remote, format!("refs/heads/{branch}")))
        }
        MutationRequest::DeleteRemoteBranch { remote, branch } => {
            Some((remote.clone(), format!("refs/heads/{branch}")))
        }
        _ => None,
    }
}

fn operation_tracks_worktree(request: &MutationRequest) -> bool {
    matches!(
        request,
        MutationRequest::CherryPick { .. } | MutationRequest::Reset { .. }
    )
}

fn operation_tracks_cherry_pick(request: &MutationRequest) -> bool {
    matches!(request, MutationRequest::CherryPick { .. })
}

fn reset_mode_name(mode: OperationResetMode) -> &'static str {
    match mode {
        OperationResetMode::Soft => "soft",
        OperationResetMode::Mixed => "mixed",
        OperationResetMode::Hard => "hard",
    }
}

fn git_reset_mode(mode: OperationResetMode) -> GitResetMode {
    match mode {
        OperationResetMode::Soft => GitResetMode::Soft,
        OperationResetMode::Mixed => GitResetMode::Mixed,
        OperationResetMode::Hard => GitResetMode::Hard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        clone_bare_repo, commit_all, configure_user, create_branch, init_repo, set_remote_head,
        set_upstream, write_file, TestRepo,
    };
    use std::{fs, path::Path};
    use tempfile::TempDir;

    fn test_repository(root: &Path) -> (TestRepo, GitClient, String, String) {
        let repo_path = root.join("worktree");
        let repo = init_repo(&repo_path, "main");
        configure_user(&repo);
        write_file(&repo_path, "README.md", "base\n");
        let base = commit_all(&repo, "base");
        create_branch(&repo, "source", &base);
        crate::test_support::checkout_branch(&repo, "source");
        write_file(&repo_path, "source.txt", "source change\n");
        let source = commit_all(&repo, "source change");
        crate::test_support::checkout_branch(&repo, "main");
        create_branch(&repo, "merged", &base);

        let origin_path = root.join("origin.git");
        let origin = clone_bare_repo(&repo_path, &origin_path);
        set_remote_head(&origin, "refs/heads/main");
        repo.remote("origin", origin_path.to_str().unwrap())
            .unwrap();
        (repo, GitClient::from_path(repo_path), base, source)
    }

    #[test]
    fn verification_rejects_missing_postconditions_for_each_mutation_class() {
        let temp = TempDir::new().unwrap();
        let (_repo, client, base, source) = test_repository(temp.path());
        let requests = vec![
            MutationRequest::CreateBranch {
                name: "new-branch".into(),
                from: Some("main".into()),
            },
            MutationRequest::Checkout {
                target: "source".into(),
            },
            MutationRequest::CheckoutDetached {
                target: source.clone(),
            },
            MutationRequest::Switch {
                target: "source".into(),
            },
            MutationRequest::CherryPick {
                source: source.clone(),
                destination: "main".into(),
            },
            MutationRequest::Reset {
                target: base.clone(),
                mode: OperationResetMode::Soft,
            },
            MutationRequest::DeleteLocalBranch {
                branch: "source".into(),
            },
            MutationRequest::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "main".into(),
            },
            MutationRequest::Cleanup {
                base: "main".into(),
                branches: Some(vec!["merged".into()]),
                exclusions: Vec::new(),
                remotes: Vec::new(),
            },
            MutationRequest::Fetch {
                remote: Some("origin".into()),
            },
            MutationRequest::Pull {
                remote: Some("origin".into()),
                branch: Some("main".into()),
            },
            MutationRequest::Push {
                remote: Some("origin".into()),
                branch: Some("main".into()),
            },
        ];
        let missing_state = OperationState::default();
        let empty_refs = BTreeMap::new();
        let empty_tracking = BTreeMap::new();

        for request in requests {
            let plan = plan_mutation(&client, &request, &OperationPreconditions::default())
                .unwrap_or_else(|error| panic!("{} plan failed: {error}", request.name()));
            let mut refs_after = BTreeMap::new();
            if matches!(request, MutationRequest::DeleteLocalBranch { .. }) {
                refs_after.insert("refs/heads/source".into(), Some(source.clone()));
            }
            if matches!(request, MutationRequest::Cleanup { .. }) {
                refs_after.insert("refs/heads/merged".into(), Some(base.clone()));
            }
            let remote_target = execution_remote_target(&request, &plan.observed_state);
            let remote_after = match &request {
                MutationRequest::DeleteRemoteBranch { .. } => Some(base.clone()),
                MutationRequest::Push { .. } => Some("f".repeat(40)),
                _ => None,
            };
            let error = verify_mutation(
                &client,
                &request,
                &plan,
                MutationPostState {
                    after: &missing_state,
                    refs_after: &refs_after,
                    tracking_after: &empty_tracking,
                    remote_target: remote_target.as_ref(),
                    remote_after: remote_after.as_deref(),
                },
            )
            .expect_err("incomplete post-operation state was accepted");
            assert!(
                matches!(error, GitError::VerificationFailed(_)),
                "{} returned an unexpected verification error: {error}",
                request.name()
            );
        }

        let checkout = MutationRequest::Checkout {
            target: "source".into(),
        };
        let checkout_plan =
            plan_mutation(&client, &checkout, &OperationPreconditions::default()).unwrap();
        let mut attached_after = checkout_plan.observed_state.clone();
        attached_after.head = checkout_plan.expected_local_effects[0].commit_id.clone();
        attached_after.branch = Some("main".into());
        assert!(verify_mutation(
            &client,
            &checkout,
            &checkout_plan,
            MutationPostState {
                after: &attached_after,
                refs_after: &empty_refs,
                tracking_after: &empty_tracking,
                remote_target: None,
                remote_after: None,
            },
        )
        .is_err());

        let detached = MutationRequest::CheckoutDetached {
            target: source.clone(),
        };
        let detached_plan =
            plan_mutation(&client, &detached, &OperationPreconditions::default()).unwrap();
        let mut attached_after = detached_plan.observed_state.clone();
        attached_after.head = detached_plan.expected_local_effects[0].commit_id.clone();
        attached_after.branch = Some("main".into());
        assert!(verify_mutation(
            &client,
            &detached,
            &detached_plan,
            MutationPostState {
                after: &attached_after,
                refs_after: &empty_refs,
                tracking_after: &empty_tracking,
                remote_target: None,
                remote_after: None,
            },
        )
        .is_err());

        let cherry_pick = MutationRequest::CherryPick {
            source: source.clone(),
            destination: "main".into(),
        };
        let cherry_pick_plan =
            plan_mutation(&client, &cherry_pick, &OperationPreconditions::default()).unwrap();
        let mut in_progress = cherry_pick_plan.observed_state.clone();
        in_progress.branch = Some("main".into());
        in_progress.head = Some(source.clone());
        in_progress.cherry_pick_head = Some(source.clone());
        let mut cherry_pick_refs = BTreeMap::new();
        cherry_pick_refs.insert("refs/heads/main".into(), Some(source.clone()));
        assert!(verify_mutation(
            &client,
            &cherry_pick,
            &cherry_pick_plan,
            MutationPostState {
                after: &in_progress,
                refs_after: &cherry_pick_refs,
                tracking_after: &empty_tracking,
                remote_target: None,
                remote_after: None,
            },
        )
        .is_err());

        let reset = MutationRequest::Reset {
            target: source,
            mode: OperationResetMode::Soft,
        };
        let reset_plan =
            plan_mutation(&client, &reset, &OperationPreconditions::default()).unwrap();
        let mut reset_refs = BTreeMap::new();
        reset_refs.insert("refs/heads/main".into(), Some(base));
        assert!(verify_mutation(
            &client,
            &reset,
            &reset_plan,
            MutationPostState {
                after: &reset_plan.observed_state,
                refs_after: &reset_refs,
                tracking_after: &empty_tracking,
                remote_target: None,
                remote_after: None,
            },
        )
        .is_err());
    }

    #[test]
    fn planning_rejects_invalid_context_and_handles_detached_checkout_targets() {
        let temp = TempDir::new().unwrap();
        let (repo, client, base, source) = test_repository(temp.path());
        let preconditions = OperationPreconditions::default();

        let checkout = plan_mutation(
            &client,
            &MutationRequest::Checkout {
                target: source.clone(),
            },
            &preconditions,
        )
        .unwrap();
        assert_eq!(
            checkout.expected_local_effects[0].action,
            "checkout_detached"
        );

        assert!(plan_mutation(
            &client,
            &MutationRequest::CreateBranch {
                name: "main".into(),
                from: None,
            },
            &preconditions,
        )
        .is_err());
        assert!(plan_mutation(
            &client,
            &MutationRequest::CreateBranch {
                name: "invalid branch name".into(),
                from: None,
            },
            &preconditions,
        )
        .is_err());

        let pull_without_upstream = plan_mutation(
            &client,
            &MutationRequest::Pull {
                remote: None,
                branch: None,
            },
            &preconditions,
        )
        .unwrap();
        assert_eq!(
            pull_without_upstream.expected_local_effects[0].commit_id,
            None
        );
        let revision_plan = plan_mutation(
            &client,
            &MutationRequest::Reset {
                target: "HEAD~0".into(),
                mode: OperationResetMode::Mixed,
            },
            &preconditions,
        )
        .unwrap();
        assert!(!revision_plan.expected_local_effects.is_empty());
        let branch_revision_plan = plan_mutation(
            &client,
            &MutationRequest::Reset {
                target: "main".into(),
                mode: OperationResetMode::Mixed,
            },
            &preconditions,
        )
        .unwrap();
        assert!(!branch_revision_plan.refs.is_empty());

        write_file(
            temp.path().join("worktree").as_path(),
            "dirty.txt",
            "dirty\n",
        );
        assert!(plan_mutation(
            &client,
            &MutationRequest::CherryPick {
                source: source.clone(),
                destination: "main".into(),
            },
            &preconditions,
        )
        .is_err());
        fs::remove_file(temp.path().join("worktree/dirty.txt")).unwrap();

        write_file(
            temp.path().join("worktree").as_path(),
            ".gitignore",
            "source.txt\n",
        );
        commit_all(&repo, "ignore source path");
        write_file(
            temp.path().join("worktree").as_path(),
            "source.txt",
            "preserve ignored file\n",
        );
        let blocked = execute_mutation(
            &client,
            &MutationRequest::CherryPick {
                source: source.clone(),
                destination: "main".into(),
            },
            &preconditions,
        )
        .unwrap();
        assert_eq!(
            blocked.failure.as_ref().map(|failure| failure.kind),
            Some(OperationFailureKind::Execution)
        );
        assert_eq!(
            blocked.receipt.verification.status,
            VerificationStatus::NotVerified
        );
        assert!(!blocked.receipt.state_changed);

        execute_mutation(
            &client,
            &MutationRequest::CheckoutDetached { target: source },
            &preconditions,
        )
        .unwrap();
        assert!(plan_mutation(
            &client,
            &MutationRequest::Reset {
                target: base,
                mode: OperationResetMode::Hard,
            },
            &preconditions,
        )
        .is_err());
    }

    #[test]
    fn stale_state_helpers_preserve_context_and_reference_details() {
        let expected = OperationState {
            head: Some("old-head".into()),
            branch: Some("main".into()),
            upstream: Some("origin/main".into()),
            ..OperationState::default()
        };
        let observed = OperationState {
            head: Some("new-head".into()),
            branch: Some("feature".into()),
            upstream: None,
            ..OperationState::default()
        };
        let error = precondition_error(&expected, &observed);
        assert!(matches!(
            error,
            GitError::PreconditionChanged { details }
                if details.expected_head.as_deref() == Some("old-head")
                    && details.expected_branch.as_deref() == Some("main")
                    && details.expected_upstream.as_deref() == Some("origin/main")
                    && details.observed_head.as_deref() == Some("new-head")
                    && details.observed_branch.as_deref() == Some("feature")
                    && details.observed_upstream.is_none()
                    && details.changed_reference.is_none()
        ));

        let temp = TempDir::new().unwrap();
        let (_repo, client, _, _) = test_repository(temp.path());
        let plan = plan_mutation(
            &client,
            &MutationRequest::Switch {
                target: "main".into(),
            },
            &OperationPreconditions::default(),
        )
        .unwrap();
        let mut refs = read_relevant_refs(&client, &plan).unwrap();
        refs.insert("refs/heads/main".into(), None);
        let error = validate_plan_refs(&plan, &refs, &plan.observed_state).unwrap_err();
        assert!(matches!(
            error,
            GitError::PreconditionChanged { details }
                if details.changed_reference.as_deref() == Some("refs/heads/main")
                    && details.expected_reference_commit_id.is_some()
                    && details.observed_reference_commit_id.is_none()
        ));
    }

    #[test]
    fn pull_and_push_target_selection_uses_explicit_and_upstream_context() {
        let upstream_state = OperationState {
            branch: Some("local".into()),
            upstream: Some("upstream/release".into()),
            ..OperationState::default()
        };
        assert_eq!(
            pull_target(None, None, &upstream_state),
            Some(("upstream".into(), "release".into()))
        );
        assert_eq!(
            pull_target(Some("origin"), None, &upstream_state),
            Some(("origin".into(), "release".into()))
        );
        assert_eq!(
            pull_target(Some("origin"), Some("main"), &upstream_state),
            Some(("origin".into(), "main".into()))
        );
        assert_eq!(
            push_target(None, None, &upstream_state),
            Some(("upstream".into(), "release".into()))
        );
        assert_eq!(
            push_target(Some("origin"), Some("main"), &upstream_state),
            Some(("origin".into(), "main".into()))
        );
        let malformed = OperationState {
            upstream: Some("missing-slash".into()),
            ..OperationState::default()
        };
        assert_eq!(pull_target(None, None, &malformed), None);
        assert_eq!(push_target(None, None, &malformed), None);
    }

    #[test]
    fn remote_and_worktree_snapshots_validate_upstream_and_fetch_refs() {
        let temp = TempDir::new().unwrap();
        let (repo, client, base, source) = test_repository(temp.path());
        for (branch, commit_id) in [
            ("main", base.clone()),
            ("merged", base.clone()),
            ("source", source),
        ] {
            let reference = format!("refs/remotes/origin/{branch}");
            repo.reference(&reference, commit_id, true, "test tracking ref")
                .unwrap();
        }
        set_upstream(&repo, "main", "origin/main");

        let state = read_state(&client, true, false).unwrap();
        assert_eq!(state.upstream.as_deref(), Some("origin/main"));
        let tracking = read_remote_tracking_refs(&client, None).unwrap();
        assert_eq!(tracking.len(), 3);
        verify_fetch_refs(&client, None, &tracking).unwrap();
        assert!(matches!(
            verify_fetch_refs(&client, Some("missing"), &tracking),
            Err(GitError::VerificationFailed(_))
        ));
        assert_eq!(
            read_remote_ref(&client, "origin", "refs/heads/main")
                .unwrap()
                .as_deref(),
            Some(base.as_str())
        );
        assert!(read_remote_ref(&client, "missing", "refs/heads/main").is_err());
    }
}
