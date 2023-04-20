use std::env;
use std::fmt::Display;
use std::process::Command;

use git2::BranchType::Local;
use git2::ResetType::Hard;
use git2::{Cred, ObjectType, PushOptions, Reference, RemoteCallbacks, Repository};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;

use crate::git::remote::GitRemote;

pub(crate) trait RepositoryOps {
    fn rebase(&self, pr: &PullRequest) -> bool;

    fn get_origin_remote(&self) -> GitRemote;

    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S);

    fn is_safe_pr(&self, pr: &PullRequest) -> bool;

    fn push(&self, pr: &PullRequest) -> bool;
}

pub(crate) struct GitRepository<'repo> {
    pub repository: &'repo mut Repository,
    has_changes_to_unstash: bool,
}

impl GitRepository<'_> {
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

    pub(crate) fn new(repository: &mut Repository) -> GitRepository {
        let has_changes_to_unstash = repository
            .stash_save2(
                &repository.signature().expect("signature should not fail"),
                None,
                None,
            )
            .is_ok();
        GitRepository {
            repository,
            has_changes_to_unstash,
        }
    }

    fn log_count(&self, since: &str, until: &str) -> usize {
        let mut revwalk = self.repository.revwalk().unwrap();

        revwalk.hide_ref(since).unwrap();
        revwalk.push_ref(until).unwrap();

        revwalk.count()
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

impl Drop for GitRepository<'_> {
    fn drop(&mut self) {
        if self.has_changes_to_unstash {
            self.repository
                .stash_pop(0, None)
                .expect("git stash pop should not fail");
        }
    }
}

impl RepositoryOps for GitRepository<'_> {
    fn rebase(&self, pr: &PullRequest) -> bool {
        let head = &pr.head.ref_field;
        let base = &pr.base.ref_field;

        let pr_title = pr.title.as_ref().unwrap();

        info!("Rebasing \"{pr_title}\" {base} <- {head}...");

        let output = Command::new("git")
            .arg("rebase")
            .arg(base)
            .arg(head)
            .output()
            .unwrap();

        let success = output.status.success();

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

    fn get_origin_remote(&self) -> GitRemote {
        GitRemote::new(self.repository.find_remote("origin").unwrap())
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
                debug!("Error finding local base branch {base}: {e}");
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
                debug!("Error finding local head branch {base}: {e}");
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

    fn push(&self, pr: &PullRequest) -> bool {
        let head = &pr.head.ref_field;

        let local_head_branch = self.repository.find_branch(head, Local).unwrap();

        let upstream_head_branch = local_head_branch.upstream().unwrap();

        let upstream_head_ref = upstream_head_branch.get();

        let pr_title = pr.title.as_ref().unwrap();

        if local_head_branch.get() == upstream_head_ref {
            info!("No changes for \"{pr_title}\". Not pushing to remote.");
            return false;
        }

        debug!("Pushing changes to remote...");

        let mut remote = self
            .repository
            .find_remote(
                self.repository
                    .branch_upstream_remote(local_head_branch.into_reference().name().unwrap())
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap();

        let mut options = PushOptions::new();

        options.remote_callbacks(remote_callbacks());

        match remote.push(&[format!("+refs/heads/{head}")], Some(&mut options)) {
            Ok(()) => {
                info!("Successfully pushed changes to remote for \"{pr_title}\"");
                true
            }
            Err(e) => {
                error!("Push to remote failed for \"{pr_title}\": {e}. Resetting...");

                let upstream_commit = upstream_head_ref.peel_to_commit().unwrap();

                self.repository
                    .reset(upstream_commit.as_object(), Hard, None)
                    .unwrap();

                info!("Successfully reset.");

                false
            }
        }
    }
}

fn remote_callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_ed25519", env::var("HOME").unwrap())),
            None,
        )
    });
    callbacks
}
