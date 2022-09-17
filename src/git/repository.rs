use std::fmt::Display;
use std::process::Command;

use crate::github::{Github, GithubClient};
use crate::{GitRemote, GitRemoteOps};
use git2::build::CheckoutBuilder;
use git2::BranchType::Local;
use git2::{
    Error, ErrorCode, Object, ObjectType, RebaseOperationType, Reference, Repository, ResetType,
};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;

pub(crate) trait RepositoryOps {
    fn rebase(&self, pr: &PullRequest) -> bool;

    fn log_count(&self, since: &str, until: &str) -> usize;

    fn switch(&self, reference: &Reference);

    fn get_primary_remote(&self) -> GitRemote;

    fn head(&self) -> Reference<'_>;

    fn resolve_reference_from_short_name(&self, refname: &str) -> Result<Reference<'_>, Error>;

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    );

    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S);

    fn is_safe_pr(&self, pr: &PullRequest) -> bool;
}

pub(crate) struct GitRepository<'repo> {
    pub(crate) repository: &'repo Repository,
}

impl GitRepository<'_> {
    pub(crate) fn with_revert_to_current_branch<F: FnMut()>(&self, mut f: F) {
        let current_head = self.head();

        let name = current_head.name().unwrap();

        debug!("Current HEAD is {name}");

        f();

        debug!("Current HEAD is {}", self.head().name().unwrap());

        let reference = self.resolve_reference_from_short_name(name).unwrap();

        self.switch(&reference);

        debug!("Current HEAD is {}", self.head().name().unwrap());
    }

    fn compare_refs(&self, head: &Reference, base: &Reference) -> (usize, usize) {
        let head_commit_name = head.name().unwrap();
        let base_commit_name = base.name().unwrap();

        (
            self.log_count(base_commit_name, head_commit_name),
            self.log_count(head_commit_name, base_commit_name),
        )
    }
}

pub(crate) struct GitRepo<'repo> {
    pub(crate) repository: &'repo GitRepository<'repo>,
    pub(crate) primary_remote: &'repo GitRemote<'repo>,
}

impl GitRepo<'_> {
    pub(crate) async fn get_all_my_safe_prs(&self) -> Vec<PullRequest> {
        let (host, owner, repo_name) = self.primary_remote.get_host_owner_repo_name();

        let github = GithubClient::new(&host);

        let github_repo = github.get_repo(&owner, &repo_name).await;

        debug!("Github repo: {github_repo:?}");

        self.repository
            .fast_forward(github_repo.default_branch.as_ref().unwrap());

        let all_my_open_prs = github.get_all_my_open_prs(&owner, &repo_name).await;

        let num_of_my_open_prs = all_my_open_prs.len();

        let my_safe_prs = all_my_open_prs
            .into_iter()
            .filter(|pr| self.repository.is_safe_pr(pr))
            .collect::<Vec<PullRequest>>();

        info!(
            "Going to rebase {}/{num_of_my_open_prs} safe pull requests:",
            my_safe_prs.len()
        );

        my_safe_prs.iter().for_each(|pr| {
            info!(
                "\"{}\" {} <- {}",
                pr.title.as_ref().unwrap(),
                pr.base.ref_field,
                pr.head.ref_field
            );
        });

        my_safe_prs
    }
}

impl GitRepository<'_> {
    pub(crate) fn new(repository: &Repository) -> GitRepository {
        GitRepository { repository }
    }

    fn libgit2_rebase(&self, head: &str, base: &str) -> bool {
        let mut rebase = self
            .repository
            .rebase(
                Some(
                    &self
                        .repository
                        .reference_to_annotated_commit(
                            &self
                                .repository
                                .resolve_reference_from_short_name(head)
                                .unwrap(),
                        )
                        .unwrap(),
                ),
                Some(
                    &self
                        .repository
                        .reference_to_annotated_commit(
                            &self
                                .repository
                                .resolve_reference_from_short_name(base)
                                .unwrap(),
                        )
                        .unwrap(),
                ),
                None,
                None,
            )
            .unwrap();

        debug!("Rebase operations: {}", rebase.len());

        while let Some(op) = rebase.next() {
            match op {
                Ok(operation) => match operation.kind().unwrap() {
                    RebaseOperationType::Pick => {
                        let commit = self.repository.find_commit(operation.id()).unwrap();
                        match rebase.commit(None, &commit.committer(), None) {
                            Ok(oid) => {
                                debug!("Successfully committed {}", oid)
                            }
                            Err(e) => {
                                if e.code() != ErrorCode::Applied {
                                    error!("Error committing: {}. Aborting...", e);
                                    rebase.abort().unwrap();
                                    return false;
                                }
                            }
                        };
                    }
                    RebaseOperationType::Reword => {
                        panic!("Reword encountered");
                    }
                    RebaseOperationType::Edit => {
                        panic!("Edit encountered");
                    }
                    RebaseOperationType::Squash => {
                        panic!("Squash encountered");
                    }
                    RebaseOperationType::Fixup => {
                        panic!("Fixup encountered");
                    }
                    RebaseOperationType::Exec => {
                        panic!("Exec encountered");
                    }
                },
                Err(e) => {
                    error!("Error rebasing {head} onto {base}: {e}. Aborting...");
                    rebase.abort().unwrap();
                    return false;
                }
            }
        }

        rebase.finish(None).unwrap();

        info!("Successfully rebased.");

        true
    }

    fn native_rebase(&self, head: &str, base: &str) -> bool {
        let output = Command::new("git")
            .arg("rebase")
            .arg(base)
            .arg(head)
            .output()
            .unwrap();

        debug!("Native rebase: {:?}", output);

        let success = output.status.success();

        if !success {
            error!("Error rebasing {head} onto {base}. Aborting...");

            assert!(Command::new("git")
                .arg("rebase")
                .arg("--abort")
                .output()
                .unwrap()
                .status
                .success());
        }

        success
    }
}

impl RepositoryOps for GitRepository<'_> {
    fn rebase(&self, pr: &PullRequest) -> bool {
        let head = &pr.head.ref_field;
        let base = &pr.base.ref_field;

        let pr_title = pr.title.as_ref().unwrap();

        info!("Rebasing \"{pr_title}\" {base} <- {head}...");

        if cfg!(feature = "native-rebase") {
            self.native_rebase(head, base)
        } else {
            self.libgit2_rebase(head, base)
        }
    }

    fn log_count(&self, since: &str, until: &str) -> usize {
        let mut revwalk = self.repository.revwalk().unwrap();

        revwalk.hide_ref(since).unwrap();
        revwalk.push_ref(until).unwrap();

        revwalk.into_iter().count()
    }

    fn switch(&self, reference: &Reference) {
        self.repository
            .checkout_tree(&reference.peel(ObjectType::Tree).unwrap(), None)
            .unwrap();
        self.repository.set_head(reference.name().unwrap()).unwrap();
    }

    fn get_primary_remote(&self) -> GitRemote {
        let remotes_array = self.repository.remotes().unwrap();

        let remotes = remotes_array
            .iter()
            .map(|it| it.unwrap())
            .collect::<Vec<&str>>();

        let primary_remote = match remotes.len() {
            1 => self.repository.find_remote(remotes[0]).unwrap(),
            2 => {
                let _origin_remote = remotes.iter().find(|&&remote| remote == "origin").unwrap();
                let upstream_remote = remotes
                    .iter()
                    .find(|&&remote| remote == "upstream")
                    .unwrap();
                self.repository.find_remote(upstream_remote).unwrap()
            }
            _ => panic!("Only 1 or 2 remotes supported."),
        };

        info!("Primary remote: {}", primary_remote.name().unwrap());

        GitRemote::new(primary_remote)
    }

    fn head(&self) -> Reference<'_> {
        self.repository.head().unwrap()
    }

    fn resolve_reference_from_short_name(&self, refname: &str) -> Result<Reference<'_>, Error> {
        self.repository.resolve_reference_from_short_name(refname)
    }

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    ) {
        self.repository.reset(target, kind, checkout).unwrap()
    }

    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S) {
        let mut local_branch = self
            .repository
            .find_branch(refname.as_ref(), Local)
            .unwrap();

        let upstream_branch = local_branch.upstream().unwrap();

        let local_reference = local_branch.get_mut();

        let upstream_reference = upstream_branch.get();

        let upstream_annotated_commit = self
            .repository
            .reference_to_annotated_commit(upstream_reference)
            .unwrap();

        let (merge_analysis, _) = self
            .repository
            .merge_analysis_for_ref(local_reference, &[&upstream_annotated_commit])
            .unwrap();

        if merge_analysis.is_up_to_date() {
            return;
        }

        if !merge_analysis.is_fast_forward() {
            panic!("Unexpected merge_analysis={merge_analysis:?}");
        }

        if *local_reference == self.repository.head().unwrap() {
            self.repository
                .checkout_tree(&upstream_reference.peel(ObjectType::Tree).unwrap(), None)
                .unwrap();
            debug!("Updated index and tree");
        }

        local_reference
            .set_target(
                upstream_reference.peel(ObjectType::Commit).unwrap().id(),
                format!("Fast-forward {refname}").as_str(),
            )
            .unwrap();

        info!("Fast-forwarded {refname}");
    }

    fn is_safe_pr(&self, pr: &PullRequest) -> bool {
        let base = &pr.base.ref_field;

        let local_base_branch = match self.repository.find_branch(base, Local) {
            Ok(branch) => branch,
            Err(e) => {
                error!("Error finding local base branch {base}: {e}");
                return false;
            }
        };

        let local_base_ref = local_base_branch.get();

        let pr_title = pr.title.as_ref().unwrap();

        if local_base_ref != local_base_branch.upstream().unwrap().get() {
            debug!("Pr \"{pr_title}\" is not safe because base ref \"{base}\" is not safe");
            return false;
        }

        let head = &pr.head.ref_field;

        let local_head_branch = match self.repository.find_branch(head, Local) {
            Ok(branch) => branch,
            Err(e) => {
                error!("Error finding local head branch {base}: {e}");
                return false;
            }
        };

        let local_head_ref = local_head_branch.get();

        if local_head_ref != local_head_branch.upstream().unwrap().get() {
            debug!("Pr \"{pr_title}\" is not safe because head ref \"{head}\" is not safe");
            return false;
        }

        debug!("\"{pr_title}\" {base} <- {head}");

        let (number_of_commits_ahead, number_of_commits_behind) =
            self.compare_refs(local_head_ref, local_base_ref);

        debug!(
        "\"{head}\" is {number_of_commits_ahead} commits ahead, {number_of_commits_behind} commits behind \"{base}\""
    );

        true
    }
}
