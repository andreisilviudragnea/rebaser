use git2::{
    Cred, Error, FetchOptions, ObjectType, PushOptions, RebaseOperationType, Remote,
    RemoteCallbacks, Repository,
};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
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

    origin_remote.fetch(
        &[format!(
            "+refs/heads/*:refs/remotes/{}/*",
            origin_remote.name().unwrap()
        )],
        Some(&mut fetch_options),
        None,
    )
}

pub(crate) fn push(origin_remote: &mut Remote, head_ref: &str) -> Result<(), Error> {
    let mut options = PushOptions::new();

    options.remote_callbacks(credentials_callback());

    origin_remote.push(&[format!("+refs/heads/{}", head_ref)], Some(&mut options))
}

pub(crate) fn rebase(pr: &PullRequest, repo: &Repository) -> bool {
    let head_ref = &pr.head.ref_field;
    let base_refname = &pr.base.ref_field;
    info!(
        "Rebasing \"{}\" {} <- {}...",
        pr.title, base_refname, head_ref
    );

    let head = repo.resolve_reference_from_short_name(head_ref).unwrap();
    let base = repo
        .resolve_reference_from_short_name(base_refname)
        .unwrap();

    let mut rebase = repo
        .rebase(
            Some(&repo.reference_to_annotated_commit(&head).unwrap()),
            Some(&repo.reference_to_annotated_commit(&base).unwrap()),
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
                        error!("Error committing for {}: {}. Aborting...", pr.title, e);
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
                error!("Error rebasing {}: {}. Aborting...", pr.title, e);
                rebase.abort().unwrap();
                return false;
            }
        }
    }

    rebase.finish(None).unwrap();

    info!(
        "Successfully rebased \"{}\". Pushing changes to remote...",
        pr.title
    );

    true
}

pub(crate) fn fast_forward_master(repo: &Repository) {
    let mut master_reference = repo.resolve_reference_from_short_name("master").unwrap();

    let origin_master_reference = repo
        .resolve_reference_from_short_name("origin/master")
        .unwrap();

    let origin_master_annotated_commit = repo
        .reference_to_annotated_commit(&origin_master_reference)
        .unwrap();

    let (merge_analysis, _) = repo
        .merge_analysis_for_ref(&master_reference, &[&origin_master_annotated_commit])
        .unwrap();

    if merge_analysis.is_up_to_date() {
        return;
    }

    if !merge_analysis.is_fast_forward() {
        panic!("Unexpected merge_analysis={:?}", merge_analysis);
    }

    info!("Fast-forwarded master");

    let origin_tree = origin_master_reference.peel(ObjectType::Tree).unwrap();

    repo.checkout_tree(&origin_tree, None).unwrap();

    master_reference
        .set_target(
            origin_master_reference
                .peel(ObjectType::Commit)
                .unwrap()
                .id(),
            "Fast forward master",
        )
        .unwrap();
}

pub(crate) fn log_count(repo: &Repository, since: &str, until: &str) -> usize {
    let mut revwalk = repo.revwalk().unwrap();

    revwalk.hide_ref(since).unwrap();
    revwalk.push_ref(until).unwrap();

    revwalk.into_iter().count()
}
