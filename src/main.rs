use std::collections::HashMap;
use std::env;

use git2::build::CheckoutBuilder;
use git2::ResetType::Hard;
use git2::{
    Cred, Error, FetchOptions, PushOptions, RebaseOperationType, Remote, RemoteCallbacks,
    Repository,
};
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;
use regex::Regex;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let repo = Repository::discover(".")?;

    let mut origin_remote = repo.find_remote("origin")?;

    fetch(&mut origin_remote);

    let remote_url = origin_remote.url().unwrap();
    println!("Origin remote: {}", remote_url);

    let regex = Regex::new(r".*@.*:(.*)/(.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    let owner = &captures[1];
    let repo_name = &captures[2];
    println!("Remote repo: {}/{}", owner, repo_name);

    let mut settings = config::Config::default();
    settings
        .merge(config::Environment::with_prefix("GITHUB"))
        .unwrap();

    let map = settings.try_into::<HashMap<String, String>>().unwrap();
    println!("{:?}", map);

    let octocrab = octocrab::OctocrabBuilder::new()
        .personal_token(map.get("oauth").unwrap().clone())
        .build()
        .unwrap();

    let user = octocrab.current().user().await.unwrap();

    let pull_request_handler = octocrab.pulls(owner, repo_name);

    let page = pull_request_handler
        .list()
        .state(State::Open)
        .send()
        .await
        .unwrap();

    let my_open_prs = page
        .items
        .into_iter()
        .filter(|it| it.user == user)
        .collect::<Vec<PullRequest>>();

    my_open_prs.iter().for_each(|pr| describe(pr, &repo));

    let safe_prs = my_open_prs
        .iter()
        .filter(|pr| is_safe_pr(&repo, pr))
        .collect::<Vec<&PullRequest>>();

    println!();

    println!(
        "Going to rebase {}/{} safe pull requests:",
        safe_prs.len(),
        my_open_prs.len()
    );

    safe_prs.iter().for_each(|pr| {
        println!(
            "\"{}\" {} <- {}",
            pr.title, pr.base.ref_field, pr.head.ref_field
        );
    });

    println!();

    loop {
        let mut changes_propagated = false;

        safe_prs.iter().for_each(|pr| {
            changes_propagated = rebase(pr, &repo, &mut origin_remote) || changes_propagated;
            println!()
        });

        if !changes_propagated {
            break;
        }
    }

    Ok(())
}

fn rebase(pr: &PullRequest, repo: &Repository, origin_remote: &mut Remote) -> bool {
    with_revert_to_current_branch(&repo, || {
        let head_ref = &pr.head.ref_field;
        let base_ref = &pr.base.ref_field;
        println!("Rebasing \"{}\" {} <- {}...", pr.title, base_ref, head_ref);

        repo.set_head(&format!("refs/heads/{}", head_ref)).unwrap();
        let mut checkout_builder = CheckoutBuilder::new();
        checkout_builder.force();
        repo.checkout_head(Some(&mut checkout_builder)).unwrap();

        let head = repo.head().unwrap();
        println!("Current HEAD is {}", head.name().unwrap());

        let reference = repo.resolve_reference_from_short_name(base_ref).unwrap();

        let mut rebase = repo
            .rebase(
                None,
                Some(&repo.reference_to_annotated_commit(&reference).unwrap()),
                None,
                None,
            )
            .unwrap();

        println!("Rebase operations: {}", rebase.len());

        let head_commit = head.peel_to_commit().unwrap();
        let signature = head_commit.committer();

        loop {
            match rebase.next() {
                Some(op) => match op {
                    Ok(operation) => match operation.kind().unwrap() {
                        RebaseOperationType::Pick => match rebase.commit(None, &signature, None) {
                            Ok(oid) => {
                                println!("Successfully committed {}", oid)
                            }
                            Err(e) => {
                                println!("Error committing for {}: {}. Aborting...", pr.title, e);
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
                        println!("Error rebasing {}: {}. Aborting...", pr.title, e);
                        rebase.abort().unwrap();
                        return false;
                    }
                },
                None => break,
            }
        }

        rebase.finish(None).unwrap();

        if is_safe_branch(repo, head_ref) {
            println!("No changes for \"{}\". Not pushing to remote.", pr.title);
            return false;
        }

        println!(
            "Successfully rebased \"{}\". Pushing changes to remote...",
            pr.title
        );

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
    })
}

fn with_revert_to_current_branch<F: FnMut() -> bool>(repo: &Repository, mut f: F) -> bool {
    let current_head = repo.head().unwrap();
    let current_head_name = current_head.name().unwrap();
    println!("Current HEAD is {}", current_head_name);

    let result = f();

    repo.set_head(current_head_name).unwrap();
    let mut checkout_builder = CheckoutBuilder::new();
    checkout_builder.force();
    repo.checkout_head(Some(&mut checkout_builder)).unwrap();

    let head = repo.head().unwrap();
    println!("Current HEAD is {}", head.name().unwrap());

    result
}

fn is_safe_pr(repo: &Repository, pr: &PullRequest) -> bool {
    let base_ref = &pr.base.ref_field;

    if !is_safe_branch(repo, base_ref) {
        println!(
            "Pr \"{}\" is not safe because base ref \"{}\" is not safe",
            pr.title, base_ref
        );
        return false;
    }

    let head_ref = &pr.head.ref_field;

    if !is_safe_branch(repo, head_ref) {
        println!(
            "Pr \"{}\" is not safe because head ref \"{}\" is not safe",
            pr.title, head_ref
        );
        return false;
    }

    true
}

fn is_safe_branch(repo: &Repository, refname: &str) -> bool {
    let origin_refname = &format!("origin/{}", refname);

    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, refname, origin_refname);

    if number_of_commits_ahead > 0 {
        println!(
            "Branch \"{}\" is unsafe because it is {} commits ahead \"{}\"",
            refname, number_of_commits_ahead, origin_refname
        );
        return false;
    }

    if number_of_commits_behind > 0 {
        println!(
            "Branch \"{}\" is unsafe because it is {} commits behind \"{}\"",
            refname, number_of_commits_behind, origin_refname
        );
        return false;
    }

    true
}

fn describe(pr: &PullRequest, repo: &Repository) {
    let head_ref = &pr.head.ref_field;
    let base_ref = &pr.base.ref_field;

    println!("\"{}\" {} <- {}", pr.title, base_ref, head_ref);

    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, head_ref, base_ref);

    println!(
        "\"{}\" is {} commits ahead, {} commits behind \"{}\"",
        head_ref, number_of_commits_ahead, number_of_commits_behind, base_ref
    );

    println!();
}

fn compare_refs(repo: &Repository, head_ref: &str, base_ref: &str) -> (usize, usize) {
    let head_reference = repo.resolve_reference_from_short_name(head_ref).unwrap();
    let head_commit_name = head_reference.name().unwrap();

    let base_reference = repo.resolve_reference_from_short_name(base_ref).unwrap();
    let base_commit_name = base_reference.name().unwrap();

    (
        log_count(repo, base_commit_name, head_commit_name),
        log_count(repo, head_commit_name, base_commit_name),
    )
}

fn log_count(repo: &Repository, since: &str, until: &str) -> usize {
    let mut revwalk = repo.revwalk().unwrap();

    revwalk.hide_ref(since).unwrap();
    revwalk.push_ref(until).unwrap();

    revwalk.into_iter().count()
}

fn fetch(origin_remote: &mut Remote) {
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
