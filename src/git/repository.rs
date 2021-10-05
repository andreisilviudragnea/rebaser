use std::fmt::Display;

use git2::build::CheckoutBuilder;
use git2::{
    Error, ErrorCode, Object, ObjectType, RebaseOperationType, Reference, Remote, Repository,
    ResetType,
};
use log::{debug, error, info};

use crate::git::remote::{GitRemote, GitRemoteOps};

pub(crate) trait RepositoryOps {
    fn rebase(&self, head: &Reference, base: &Reference) -> Result<bool, Error>;

    fn fast_forward<S: AsRef<str> + Display>(
        &self,
        remote: &GitRemote,
        refname: S,
    ) -> Result<(), Error>;

    fn log_count(&self, since: &str, until: &str) -> Result<usize, Error>;

    fn switch(&self, reference: &Reference) -> Result<(), Error>;

    fn get_primary_remote(&self) -> Option<Remote>;

    fn head(&self) -> Result<Reference<'_>, Error>;

    fn resolve_reference_from_short_name(&self, refname: &str) -> Result<Reference<'_>, Error>;

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    ) -> Result<(), Error>;
}

pub(crate) struct GitRepository(Repository);

impl GitRepository {
    pub(crate) fn new() -> GitRepository {
        GitRepository(Repository::discover(".").unwrap())
    }
}

impl RepositoryOps for GitRepository {
    fn rebase(&self, head: &Reference, base: &Reference) -> Result<bool, Error> {
        let mut rebase = self.0.rebase(
            Some(&self.0.reference_to_annotated_commit(head)?),
            Some(&self.0.reference_to_annotated_commit(base)?),
            None,
            None,
        )?;

        debug!("Rebase operations: {}", rebase.len());

        let head_commit = self.0.head()?.peel_to_commit()?;
        let signature = head_commit.committer();

        while let Some(op) = rebase.next() {
            match op {
                Ok(operation) => match operation.kind().unwrap() {
                    RebaseOperationType::Pick => match rebase.commit(None, &signature, None) {
                        Ok(oid) => {
                            debug!("Successfully committed {}", oid)
                        }
                        Err(e) => {
                            if e.code() != ErrorCode::Applied {
                                error!("Error committing: {}. Aborting...", e);
                                rebase.abort()?;
                                return Ok(false);
                            }
                        }
                    },
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
                    error!("Error rebasing :{}. Aborting...", e);
                    rebase.abort()?;
                    return Ok(false);
                }
            }
        }

        rebase.finish(None)?;

        info!("Successfully rebased.");

        Ok(true)
    }

    fn fast_forward<S: AsRef<str> + Display>(
        &self,
        remote: &GitRemote,
        refname: S,
    ) -> Result<(), Error> {
        let mut reference = self.0.resolve_reference_from_short_name(refname.as_ref())?;

        let remote_reference = self
            .0
            .resolve_reference_from_short_name(format!("{}/{}", remote.name(), refname).as_str())?;

        let remote_annotated_commit = self.0.reference_to_annotated_commit(&remote_reference)?;

        let (merge_analysis, _) = self
            .0
            .merge_analysis_for_ref(&reference, &[&remote_annotated_commit])?;

        if merge_analysis.is_up_to_date() {
            return Ok(());
        }

        if !merge_analysis.is_fast_forward() {
            panic!("Unexpected merge_analysis={:?}", merge_analysis);
        }

        let remote_tree = remote_reference.peel(ObjectType::Tree)?;

        self.0.checkout_tree(&remote_tree, None)?;

        reference.set_target(
            remote_reference.peel(ObjectType::Commit)?.id(),
            format!("Fast forward {}", refname).as_str(),
        )?;

        info!("Fast-forwarded {}", refname);

        Ok(())
    }

    fn log_count(&self, since: &str, until: &str) -> Result<usize, Error> {
        let mut revwalk = self.0.revwalk()?;

        revwalk.hide_ref(since)?;
        revwalk.push_ref(until)?;

        Ok(revwalk.into_iter().count())
    }

    fn switch(&self, reference: &Reference) -> Result<(), Error> {
        self.0
            .checkout_tree(&reference.peel(ObjectType::Tree)?, None)?;
        self.0.set_head(reference.name().unwrap())?;

        Ok(())
    }

    fn get_primary_remote(&self) -> Option<Remote> {
        let remotes_array = self.0.remotes().unwrap();

        let remotes = remotes_array
            .iter()
            .map(|it| it.unwrap())
            .collect::<Vec<&str>>();

        return match remotes.len() {
            1 => Some(self.0.find_remote(remotes[0]).unwrap()),
            2 => {
                let _origin_remote = remotes.iter().find(|&&remote| remote == "origin").unwrap();
                let upstream_remote = remotes
                    .iter()
                    .find(|&&remote| remote == "upstream")
                    .unwrap();
                Some(self.0.find_remote(*upstream_remote).unwrap())
            }
            _ => {
                error!("Only 1 or 2 remotes supported.");
                None
            }
        };
    }

    fn head(&self) -> Result<Reference<'_>, Error> {
        self.0.head()
    }

    fn resolve_reference_from_short_name(&self, refname: &str) -> Result<Reference<'_>, Error> {
        self.0.resolve_reference_from_short_name(refname)
    }

    fn reset(
        &self,
        target: &Object<'_>,
        kind: ResetType,
        checkout: Option<&mut CheckoutBuilder<'_>>,
    ) -> Result<(), Error> {
        self.0.reset(target, kind, checkout)
    }
}
