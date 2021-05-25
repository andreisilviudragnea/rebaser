use std::{env, fs};

use git2::ResetType::Hard;
use git2::{
    Cred, FetchOptions, ObjectType, PushOptions, RebaseOperationType, Reference, Remote,
    RemoteCallbacks, Repository,
};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;
use octocrab::{Octocrab, OctocrabBuilder};
use regex::Regex;
use std::env::var;
use toml::Value;

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

fn push(pr: &PullRequest, repo: &Repository, origin_remote: &mut Remote) -> bool {
    let mut options = PushOptions::new();

    options.remote_callbacks(credentials_callback());

    let head_ref = &pr.head.ref_field;

    match origin_remote.push(&[format!("+refs/heads/{}", head_ref)], Some(&mut options)) {
        Ok(()) => {
            info!("Successfully pushed changes to remote for \"{}\"", pr.title);
            true
        }
        Err(e) => {
            error!(
                "Push to remote failed for \"{}\": {}. Resetting...",
                pr.title, e
            );

            let origin_refname = &format!("origin/{}", head_ref);

            let origin_reference = repo
                .resolve_reference_from_short_name(origin_refname)
                .unwrap();

            let origin_commit = origin_reference.peel_to_commit().unwrap();

            repo.reset(origin_commit.as_object(), Hard, None).unwrap();

            info!("Successfully reset.");

            false
        }
    }
}

fn rebase(pr: &PullRequest, repo: &Repository) -> bool {
    let head_refname = &pr.head.ref_field;
    let base_refname = &pr.base.ref_field;
    info!(
        "Rebasing \"{}\" {} <- {}...",
        pr.title, base_refname, head_refname
    );

    let head_ref = repo
        .resolve_reference_from_short_name(head_refname)
        .unwrap();
    let base_ref = repo
        .resolve_reference_from_short_name(base_refname)
        .unwrap();

    let mut rebase = repo
        .rebase(
            Some(&repo.reference_to_annotated_commit(&head_ref).unwrap()),
            Some(&repo.reference_to_annotated_commit(&base_ref).unwrap()),
            None,
            None,
        )
        .unwrap();

    debug!("Rebase operations: {}", rebase.len());

    let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = head_commit.committer();

    loop {
        match rebase.next() {
            Some(op) => match op {
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
            },
            None => break,
        }
    }

    rebase.finish(None).unwrap();

    if is_safe_branch(repo, &head_ref, head_refname) {
        info!("No changes for \"{}\". Not pushing to remote.", pr.title);
        return false;
    }

    info!(
        "Successfully rebased \"{}\". Pushing changes to remote...",
        pr.title
    );

    true
}

fn is_safe_branch(repo: &Repository, reference: &Reference, refname: &str) -> bool {
    let origin_refname = &format!("origin/{}", refname);
    let origin = repo
        .resolve_reference_from_short_name(origin_refname)
        .unwrap();

    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, &reference, &origin);

    if number_of_commits_ahead > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits ahead \"{}\"",
            refname, number_of_commits_ahead, origin_refname
        );
        return false;
    }

    if number_of_commits_behind > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits behind \"{}\"",
            refname, number_of_commits_behind, origin_refname
        );
        return false;
    }

    true
}

fn compare_refs(repo: &Repository, head: &Reference, base: &Reference) -> (usize, usize) {
    let head_commit_name = head.name().unwrap();
    let base_commit_name = base.name().unwrap();

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

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &Repository,
    origin_remote: &mut Remote,
) -> bool {
    let result = rebase(pr, repo);

    if !result {
        return result;
    }

    push(pr, repo, origin_remote)
}

pub(crate) fn with_revert_to_current_branch<F: FnMut() -> ()>(repo: &Repository, mut f: F) {
    let current_head = repo.head().unwrap();
    let current_head_name = current_head.name().unwrap();
    debug!("Current HEAD is {}", current_head_name);

    f();

    let head = repo.head().unwrap();
    debug!("Current HEAD is {}", head.name().unwrap());

    repo.checkout_tree(&current_head.peel(ObjectType::Tree).unwrap(), None)
        .unwrap();
    repo.set_head(current_head_name).unwrap();

    let head = repo.head().unwrap();
    debug!("Current HEAD is {}", head.name().unwrap());
}

fn is_safe_pr(repo: &Repository, pr: &PullRequest) -> bool {
    let base_ref = &pr.base.ref_field;
    let base = match repo.resolve_reference_from_short_name(base_ref) {
        Ok(reference) => reference,
        Err(e) => {
            error!(
                "Error resolving reference from shortname for {}: {}",
                base_ref, e
            );
            return false;
        }
    };

    if !is_safe_branch(repo, &base, base_ref) {
        debug!(
            "Pr \"{}\" is not safe because base ref \"{}\" is not safe",
            pr.title, base_ref
        );
        return false;
    }

    let head_ref = &pr.head.ref_field;
    let head = match repo.resolve_reference_from_short_name(head_ref) {
        Ok(reference) => reference,
        Err(e) => {
            error!(
                "Error resolving reference from shortname for {}: {}",
                head_ref, e
            );
            return false;
        }
    };

    if !is_safe_branch(repo, &head, head_ref) {
        debug!(
            "Pr \"{}\" is not safe because head ref \"{}\" is not safe",
            pr.title, head_ref
        );
        return false;
    }

    debug!("\"{}\" {} <- {}", pr.title, base_ref, head_ref);

    let (number_of_commits_ahead, number_of_commits_behind) = compare_refs(repo, &head, &base);

    debug!(
        "\"{}\" is {} commits ahead, {} commits behind \"{}\"",
        head_ref, number_of_commits_ahead, number_of_commits_behind, base_ref
    );

    true
}

fn get_host_owner_repo_name(origin_remote: &Remote) -> (String, String, String) {
    let remote_url = origin_remote.url().unwrap();
    debug!("Origin remote: {}", remote_url);

    let regex = Regex::new(r".*@(.*):(.*)/(.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    let host = &captures[1];
    let owner = &captures[2];
    let repo_name = &captures[3];

    debug!("{}:{}/{}", host, owner, repo_name);

    (host.to_owned(), owner.to_owned(), repo_name.to_owned())
}

pub(crate) async fn get_all_my_safe_prs(
    repo: &Repository,
    origin_remote: &Remote<'_>,
) -> Vec<PullRequest> {
    let (host, owner, repo_name) = get_host_owner_repo_name(origin_remote);

    let octocrab = init_octocrab(&host);

    let all_prs = get_all_prs(&owner, &repo_name, &octocrab).await;

    let user = octocrab.current().user().await.unwrap();

    let my_open_prs = all_prs
        .into_iter()
        .filter(|pr| pr.user == user)
        .collect::<Vec<PullRequest>>();

    let num_of_my_open_prs = my_open_prs.len();

    let my_safe_prs = my_open_prs
        .into_iter()
        .filter(|pr| is_safe_pr(repo, pr))
        .collect::<Vec<PullRequest>>();

    info!(
        "Going to rebase {}/{} safe pull requests:",
        my_safe_prs.len(),
        num_of_my_open_prs
    );

    my_safe_prs.iter().for_each(|pr| {
        info!(
            "\"{}\" {} <- {}",
            pr.title, pr.base.ref_field, pr.head.ref_field
        );
    });

    my_safe_prs
}

fn init_octocrab(host: &String) -> Octocrab {
    let oauth_token = get_oauth_token(&host);

    OctocrabBuilder::new()
        .base_url(if host == "github.com" {
            "https://api.github.com/".to_string()
        } else {
            format!("https://{}/api/v3/", host)
        })
        .unwrap()
        .personal_token(oauth_token)
        .build()
        .unwrap()
}

fn get_oauth_token(host: &str) -> String {
    let filename = format!("{}/.github", var("HOME").unwrap());

    let config = fs::read_to_string(&filename)
        .expect(format!("File {} is missing", filename).as_str())
        .parse::<Value>()
        .expect(format!("Error parsing {}", filename).as_str());

    let config_table = config
        .as_table()
        .expect(format!("Error parsing {}", filename).as_str());

    let github_table = config_table
        .get(host)
        .expect(format!("{} table missing from {}", host, filename).as_str())
        .as_table()
        .expect(format!("Error parsing table {} from {}", host, filename).as_str());

    github_table
        .get("oauth")
        .expect(format!("Missing oauth key for {} in {}", host, filename).as_str())
        .as_str()
        .expect(
            format!(
                "Expected string for oauth key under {} in {}",
                host, filename
            )
            .as_str(),
        )
        .to_owned()
}

async fn get_all_prs(owner: &str, repo_name: &str, octocrab: &Octocrab) -> Vec<PullRequest> {
    let pull_request_handler = octocrab.pulls(owner, repo_name);

    let mut page = pull_request_handler
        .list()
        .state(State::Open)
        .send()
        .await
        .unwrap();

    let mut all_prs = page.items.into_iter().collect::<Vec<PullRequest>>();

    loop {
        match &page.next {
            None => break,
            Some(url) => {
                page = octocrab
                    .get_page(&Some(url.to_owned()))
                    .await
                    .unwrap()
                    .unwrap();

                for item in page.items {
                    all_prs.push(item)
                }
            }
        }
    }

    all_prs
}

pub(crate) fn fast_forward_master(repo: &Repository) {
    let master_reference = repo.resolve_reference_from_short_name("master").unwrap();

    let mut origin_master_reference = repo
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

    repo.checkout_tree(
        &origin_master_reference.peel(ObjectType::Tree).unwrap(),
        None,
    )
    .unwrap();

    origin_master_reference
        .set_target(
            origin_master_reference.target_peel().unwrap(),
            "Fast forward master",
        )
        .unwrap();
}
