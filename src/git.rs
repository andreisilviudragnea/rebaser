use git2::{
    Cred, Error, FetchOptions, ObjectType, PushOptions, RebaseOperationType, Reference, Remote,
    RemoteCallbacks, Repository,
};
use log::{debug, error, info};
use std::env;

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

pub(crate) fn fetch(origin_remote: &mut Remote) -> Result<(), Error> {
    let callbacks = credentials_callback();

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let remote_name = origin_remote.name().unwrap();

    origin_remote.fetch(
        &[format!("+refs/heads/*:refs/remotes/{}/*", remote_name)],
        Some(&mut fetch_options),
        Some(format!("Fetched from remote {}", remote_name).as_str()),
    )
}

pub(crate) fn push(origin_remote: &mut Remote, head_ref: &str) -> Result<(), Error> {
    let mut options = PushOptions::new();

    options.remote_callbacks(credentials_callback());

    origin_remote.push(&[format!("+refs/heads/{}", head_ref)], Some(&mut options))
}

pub(crate) fn rebase(repo: &Repository, head: &Reference, base: &Reference) -> bool {
    let mut rebase = repo
        .rebase(
            Some(&repo.reference_to_annotated_commit(head).unwrap()),
            Some(&repo.reference_to_annotated_commit(base).unwrap()),
            None,
            None,
        )
        .unwrap();

    debug!("Rebase operations: {}", rebase.len());

    let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = head_commit.committer();

    while let Some(op) = rebase.next() {
        match op {
            Ok(operation) => match operation.kind().unwrap() {
                RebaseOperationType::Pick => match rebase.commit(None, &signature, None) {
                    Ok(oid) => {
                        debug!("Successfully committed {}", oid)
                    }
                    Err(e) => {
                        error!("Error committing: {}. Aborting...", e);
                        rebase.abort().unwrap();
                        return false;
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
                rebase.abort().unwrap();
                return false;
            }
        }
    }

    rebase.finish(None).unwrap();

    info!("Successfully rebased.");

    true
}

pub(crate) fn fast_forward(repo: &Repository, refname: &str) {
    let mut reference = repo.resolve_reference_from_short_name(refname).unwrap();

    let origin_reference = repo
        .resolve_reference_from_short_name(format!("origin/{}", refname).as_str())
        .unwrap();

    let origin_annotated_commit = repo
        .reference_to_annotated_commit(&origin_reference)
        .unwrap();

    let (merge_analysis, _) = repo
        .merge_analysis_for_ref(&reference, &[&origin_annotated_commit])
        .unwrap();

    if merge_analysis.is_up_to_date() {
        return;
    }

    if !merge_analysis.is_fast_forward() {
        panic!("Unexpected merge_analysis={:?}", merge_analysis);
    }

    info!("Fast-forwarded {}", refname);

    let origin_tree = origin_reference.peel(ObjectType::Tree).unwrap();

    repo.checkout_tree(&origin_tree, None).unwrap();

    reference
        .set_target(
            origin_reference.peel(ObjectType::Commit).unwrap().id(),
            format!("Fast forward {}", refname).as_str(),
        )
        .unwrap();
}

pub(crate) fn log_count(repo: &Repository, since: &str, until: &str) -> usize {
    let mut revwalk = repo.revwalk().unwrap();

    revwalk.hide_ref(since).unwrap();
    revwalk.push_ref(until).unwrap();

    revwalk.into_iter().count()
}
