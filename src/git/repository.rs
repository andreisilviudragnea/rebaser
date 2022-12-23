use std::fmt::Display;
use std::process::Command;

use git2::build::CheckoutBuilder;
use git2::BranchType::Local;
use git2::{Branch, BranchType, Error, Object, ObjectType, Reference, Repository, ResetType};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;

use crate::git::remote::GitRemote;

pub(crate) trait RepositoryOps {
    fn rebase(&self, pr: &PullRequest) -> bool;

    fn get_primary_remote(&self) -> GitRemote;

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    );

    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S);

    fn is_safe_pr(&self, pr: &PullRequest) -> bool;

    fn find_branch(&self, name: &str, branch_type: BranchType) -> Result<Branch<'_>, Error>;
}

pub(crate) struct GitRepository {
    repository: Repository,
    has_changes_to_unstash: bool,
}

impl GitRepository {
    pub(crate) fn with_revert_to_current_branch<F: FnMut()>(&self, mut f: F) {
        let current_head = self.head();

        let name = current_head.name().unwrap();

        debug!("Current HEAD is {name}");

        f();

        debug!("Current HEAD is {}", self.head().name().unwrap());

        self.switch(current_head.shorthand().unwrap());

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

    pub(crate) fn new() -> GitRepository {
        let mut repository = Repository::discover(".").unwrap();
        let has_changes_to_unstash = match repository.stash_save2(
            &repository.signature().expect("signature should not fail"),
            None,
            None,
        ) {
            Ok(_) => true,
            Err(_) => false,
        };
        GitRepository {
            repository,
            has_changes_to_unstash,
        }
    }

    fn log_count(&self, since: &str, until: &str) -> usize {
        let mut revwalk = self.repository.revwalk().unwrap();

        revwalk.hide_ref(since).unwrap();
        revwalk.push_ref(until).unwrap();

        revwalk.into_iter().count()
    }

    fn switch(&self, name: &str) {
        let local_branch = self.repository.find_branch(name, Local).unwrap();
        let reference = local_branch.get();
        self.repository
            .checkout_tree(&reference.peel(ObjectType::Tree).unwrap(), None)
            .unwrap();
        self.repository.set_head(reference.name().unwrap()).unwrap();
    }

    fn head(&self) -> Reference<'_> {
        self.repository.head().unwrap()
    }
}

impl Drop for GitRepository {
    fn drop(&mut self) {
        if self.has_changes_to_unstash {
            self.repository
                .stash_pop(0, None)
                .expect("git stash pop should not fail");
        }
    }
}

impl RepositoryOps for GitRepository {
    fn rebase(&self, pr: &PullRequest) -> bool {
        let head = &pr.head.ref_field;
        let base = &pr.base.ref_field;

        let pr_title = pr.title.as_ref().unwrap();

        info!("Rebasing \"{pr_title}\" {base} <- {head}...");

        let status = Command::new("git")
            .arg("rebase")
            .arg(base)
            .arg(head)
            .status()
            .unwrap();

        let success = status.success();

        if !success {
            error!("Error rebasing {head} onto {base}. Aborting...");

            assert!(Command::new("git")
                .arg("rebase")
                .arg("--abort")
                .status()
                .expect("git rebase --abort should not fail")
                .success());
        }

        success
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

    fn find_branch(&self, name: &str, branch_type: BranchType) -> Result<Branch<'_>, Error> {
        self.repository.find_branch(name, branch_type)
    }
}
