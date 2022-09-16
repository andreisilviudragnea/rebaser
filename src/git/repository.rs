use std::fmt::Display;
use std::process::Command;

use git2::build::CheckoutBuilder;
use git2::{
    Error, ErrorCode, Object, ObjectType, RebaseOperationType, Reference, Remote, Repository,
    ResetType,
};
use log::{debug, error, info};

use crate::git::remote::{GitRemote, GitRemoteOps};

pub(crate) trait RepositoryOps {
    fn rebase(&self, head: &str, base: &str) -> bool;

    fn fast_forward<S: AsRef<str> + Display>(&self, remote: &GitRemote, refname: S);

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

pub(crate) struct GitRepository {
    repository: Repository
}

impl GitRepository {
    pub(crate) fn new() -> GitRepository {
        GitRepository {
            repository: Repository::discover(".").unwrap()
        }
    }

    fn libgit2_rebase(&self, head: &str, base: &str) -> bool {
        let mut rebase = self
            .repository
            .rebase(
                Some(
                    &self
                        .repository
                        .reference_to_annotated_commit(
                            &self.repository.resolve_reference_from_short_name(head).unwrap(),
                        )
                        .unwrap(),
                ),
                Some(
                    &self
                        .repository
                        .reference_to_annotated_commit(
                            &self.repository.resolve_reference_from_short_name(base).unwrap(),
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

impl RepositoryOps for GitRepository {
    fn rebase(&self, head: &str, base: &str) -> bool {
        if cfg!(feature = "native-rebase") {
            self.native_rebase(head, base)
        } else {
            self.libgit2_rebase(head, base)
        }
    }

    fn fast_forward<S: AsRef<str> + Display>(&self, remote: &GitRemote, refname: S) {
        let mut reference = self
            .repository
            .resolve_reference_from_short_name(refname.as_ref())
            .unwrap();

        let remote_reference = self
            .repository
            .resolve_reference_from_short_name(format!("{}/{refname}", remote.name()).as_str())
            .unwrap();

        let remote_annotated_commit = self
            .repository
            .reference_to_annotated_commit(&remote_reference)
            .unwrap();

        let (merge_analysis, _) = self
            .repository
            .merge_analysis_for_ref(&reference, &[&remote_annotated_commit])
            .unwrap();

        if merge_analysis.is_up_to_date() {
            return;
        }

        if !merge_analysis.is_fast_forward() {
            panic!("Unexpected merge_analysis={merge_analysis:?}");
        }

        if reference == self.head() {
            self.repository
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
