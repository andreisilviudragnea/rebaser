use std::fmt::Display;
use std::process::Command;

use crate::github::{Github, GithubClient};
use crate::{GitRemote, GitRemoteOps};
use git2::build::CheckoutBuilder;
use git2::{
    Error, ErrorCode, Object, ObjectType, RebaseOperationType, Reference, Remote, Repository,
    ResetType,
};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use regex::Regex;

pub(crate) trait RepositoryOps {
    fn rebase(&self, head: &str, base: &str) -> bool;

    fn log_count(&self, since: &str, until: &str) -> usize;

    fn switch(&self, reference: &Reference);

    fn get_primary_remote(&self) -> Remote;

    fn head(&self) -> Reference<'_>;

    fn resolve_reference_from_short_name(&self, refname: &str) -> Result<Reference<'_>, Error>;

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    );
}

pub(crate) struct GitRepository<'repo> {
    repository: &'repo Repository,
}

pub(crate) struct GitRepo<'repo> {
    pub(crate) repository: &'repo GitRepository<'repo>,
    pub(crate) primary_remote: &'repo GitRemote<'repo>,
}

impl GitRepo<'_> {
    pub(crate) async fn get_all_my_safe_prs(&self) -> Vec<PullRequest> {
        let (host, owner, repo_name) = self.get_host_owner_repo_name();

        let github = GithubClient::new(&host);

        let github_repo = github.get_repo(&owner, &repo_name).await;

        debug!("Github repo: {github_repo:?}");

        self.fast_forward(github_repo.default_branch.as_ref().unwrap());

        let all_prs = github.get_all_open_prs(&owner, &repo_name).await;

        let user = github.get_current_user().await;

        let my_open_prs = all_prs
            .into_iter()
            .filter(|pr| **pr.user.as_ref().unwrap() == user)
            .collect::<Vec<PullRequest>>();

        let num_of_my_open_prs = my_open_prs.len();

        let my_safe_prs = my_open_prs
            .into_iter()
            .filter(|pr| self.is_safe_pr(pr))
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

pub(crate) trait GitRepoOps {
    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S);

    fn get_host_owner_repo_name(&self) -> (String, String, String);

    fn is_safe_pr(&self, pr: &PullRequest) -> bool;
}

impl GitRepoOps for GitRepo<'_> {
    fn fast_forward<S: AsRef<str> + Display>(&self, refname: S) {
        let mut reference = self
            .repository
            .resolve_reference_from_short_name(refname.as_ref())
            .unwrap();

        let remote_reference = self
            .repository
            .resolve_reference_from_short_name(
                format!("{}/{refname}", self.primary_remote.name()).as_str(),
            )
            .unwrap();

        let remote_annotated_commit = self
            .repository
            .repository
            .reference_to_annotated_commit(&remote_reference)
            .unwrap();

        let (merge_analysis, _) = self
            .repository
            .repository
            .merge_analysis_for_ref(&reference, &[&remote_annotated_commit])
            .unwrap();

        if merge_analysis.is_up_to_date() {
            return;
        }

        if !merge_analysis.is_fast_forward() {
            panic!("Unexpected merge_analysis={merge_analysis:?}");
        }

        if reference == self.repository.head() {
            self.repository
                .repository
                .checkout_tree(&remote_reference.peel(ObjectType::Tree).unwrap(), None)
                .unwrap();
            debug!("Updated index and tree");
        }

        reference
            .set_target(
                remote_reference.peel(ObjectType::Commit).unwrap().id(),
                format!("Fast-forward {refname}").as_str(),
            )
            .unwrap();

        info!("Fast-forwarded {refname}");
    }

    fn get_host_owner_repo_name(&self) -> (String, String, String) {
        let remote_url = self.primary_remote.url();
        debug!("remote_url: {remote_url}");

        let regex = Regex::new(r".*@(.*):(.*)/(.*).git").unwrap();

        let captures = regex.captures(remote_url).unwrap();

        let host = &captures[1];
        let owner = &captures[2];
        let repo_name = &captures[3];

        debug!("{host}:{owner}/{repo_name}");

        (host.to_owned(), owner.to_owned(), repo_name.to_owned())
    }

    fn is_safe_pr(&self, pr: &PullRequest) -> bool {
        let base = &pr.base.ref_field;

        let base_ref = match self.repository.resolve_reference_from_short_name(base) {
            Ok(reference) => reference,
            Err(e) => {
                error!("Error resolving reference from shortname for {base}: {e}");
                return false;
            }
        };

        let remote_name = self.primary_remote.name();

        let remote_base_ref = self
            .repository
            .resolve_reference_from_short_name(&format!("{}/{base}", remote_name))
            .unwrap();

        let pr_title = pr.title.as_ref().unwrap();

        if base_ref != remote_base_ref {
            debug!("Pr \"{pr_title}\" is not safe because base ref \"{base}\" is not safe");
            return false;
        }

        let head = &pr.head.ref_field;

        let head_ref = match self.repository.resolve_reference_from_short_name(head) {
            Ok(reference) => reference,
            Err(e) => {
                error!("Error resolving reference from shortname for {head}: {e}");
                return false;
            }
        };

        let remote_head_ref = self
            .repository
            .resolve_reference_from_short_name(&format!("{}/{}", remote_name, head))
            .unwrap();

        if head_ref != remote_head_ref {
            debug!("Pr \"{pr_title}\" is not safe because head ref \"{head}\" is not safe");
            return false;
        }

        debug!("\"{pr_title}\" {base} <- {head}");

        let (number_of_commits_ahead, number_of_commits_behind) =
            compare_refs(self.repository, &head_ref, &base_ref);

        debug!(
        "\"{head}\" is {number_of_commits_ahead} commits ahead, {number_of_commits_behind} commits behind \"{base}\""
    );

        true
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
    fn rebase(&self, head: &str, base: &str) -> bool {
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

    fn get_primary_remote(&self) -> Remote {
        let remotes_array = self.repository.remotes().unwrap();

        let remotes = remotes_array
            .iter()
            .map(|it| it.unwrap())
            .collect::<Vec<&str>>();

        match remotes.len() {
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
        }
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
}

fn compare_refs(repo: &GitRepository, head: &Reference, base: &Reference) -> (usize, usize) {
    let head_commit_name = head.name().unwrap();
    let base_commit_name = base.name().unwrap();

    (
        repo.log_count(base_commit_name, head_commit_name),
        repo.log_count(head_commit_name, base_commit_name),
    )
}
