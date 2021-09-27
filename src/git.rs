use git2::{
    Cred, Error, ErrorCode, FetchOptions, ObjectType, PushOptions, RebaseOperationType, Reference,
    Remote, RemoteCallbacks, Repository,
};
use log::{debug, error, info};
use std::env;
use std::fmt::Display;

fn credentials_callback<'a>() -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_rsa", env::var("HOME").unwrap())),
            None,
        )
    });
    callbacks
}

pub(crate) fn fetch(remote: &mut Remote) -> Result<(), Error> {
    let callbacks = credentials_callback();

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let remote_name = remote.name().unwrap();

    let refspecs = format!("+refs/heads/*:refs/remotes/{}/*", remote_name);
    let reflog_msg = format!("Fetched from remote {}", remote_name);

    remote.fetch(
        &[refspecs],
        Some(&mut fetch_options),
        Some(reflog_msg.as_str()),
    )
}

pub(crate) fn push(remote: &mut Remote, head_ref: &str) -> Result<(), Error> {
    let mut options = PushOptions::new();

    options.remote_callbacks(credentials_callback());

    remote.push(&[format!("+refs/heads/{}", head_ref)], Some(&mut options))
}

pub(crate) fn rebase(repo: &Repository, head: &Reference, base: &Reference) -> Result<bool, Error> {
    let mut rebase = repo.rebase(
        Some(&repo.reference_to_annotated_commit(head)?),
        Some(&repo.reference_to_annotated_commit(base)?),
        None,
        None,
    )?;

    debug!("Rebase operations: {}", rebase.len());

    let head_commit = repo.head()?.peel_to_commit()?;
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

pub(crate) fn fast_forward<S: AsRef<str> + Display>(
    repo: &Repository,
    remote: &Remote,
    refname: S,
) -> Result<(), Error> {
    let mut reference = repo.resolve_reference_from_short_name(refname.as_ref())?;

    let remote_reference = repo.resolve_reference_from_short_name(
        format!("{}/{}", remote.name().unwrap(), refname).as_str(),
    )?;

    let remote_annotated_commit = repo.reference_to_annotated_commit(&remote_reference)?;

    let (merge_analysis, _) =
        repo.merge_analysis_for_ref(&reference, &[&remote_annotated_commit])?;

    if merge_analysis.is_up_to_date() {
        return Ok(());
    }

    if !merge_analysis.is_fast_forward() {
        panic!("Unexpected merge_analysis={:?}", merge_analysis);
    }

    let remote_tree = remote_reference.peel(ObjectType::Tree)?;

    repo.checkout_tree(&remote_tree, None)?;

    reference.set_target(
        remote_reference.peel(ObjectType::Commit)?.id(),
        format!("Fast forward {}", refname).as_str(),
    )?;

    info!("Fast-forwarded {}", refname);

    Ok(())
}

pub(crate) fn log_count(repo: &Repository, since: &str, until: &str) -> Result<usize, Error> {
    let mut revwalk = repo.revwalk()?;

    revwalk.hide_ref(since)?;
    revwalk.push_ref(until)?;

    Ok(revwalk.into_iter().count())
}

pub(crate) fn switch(repo: &Repository, reference: &Reference) -> Result<(), Error> {
    repo.checkout_tree(&reference.peel(ObjectType::Tree)?, None)?;
    repo.set_head(reference.name().unwrap())?;

    Ok(())
}
