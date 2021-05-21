use git2::ResetType::Hard;
use git2::{Cred, FetchOptions, PushOptions, Remote, RemoteCallbacks, Repository};
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

pub(crate) fn fetch(origin_remote: &mut Remote) {
    let callbacks = credentials_callback();

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    origin_remote
        .fetch(
            &[format!(
                "+refs/heads/*:refs/remotes/{}/*",
                origin_remote.name().unwrap()
            )],
            Some(&mut fetch_options),
            None,
        )
        .unwrap();
}

pub(crate) fn push(
    pr: &PullRequest,
    repo: &Repository,
    origin_remote: &mut Remote,
    head_ref: &&String,
) -> bool {
    let mut options = PushOptions::new();

    options.remote_callbacks(credentials_callback());

    match origin_remote.push(&[format!("+refs/heads/{}", head_ref)], Some(&mut options)) {
        Ok(()) => {
            println!("Successfully pushed changes to remote for \"{}\"", pr.title);
            true
        }
        Err(e) => {
            println!(
                "Push to remote failed for \"{}\": {}. Resetting...",
                pr.title, e
            );

            let origin_refname = &format!("origin/{}", head_ref);

            let origin_reference = repo
                .resolve_reference_from_short_name(origin_refname)
                .unwrap();

            let origin_commit = origin_reference.peel_to_commit().unwrap();

            repo.reset(origin_commit.as_object(), Hard, None).unwrap();

            println!("Successfully reset.");

            false
        }
    }
}
