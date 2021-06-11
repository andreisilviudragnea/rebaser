use std::env::var;
use std::fs;

use crate::git::{log_count, push, rebase};
use git2::{ObjectType, Reference, Remote, Repository};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;
use octocrab::{Octocrab, OctocrabBuilder};
use regex::Regex;
use toml::Value;

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

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &Repository,
    origin_remote: &mut Remote,
) -> bool {
    let result = rebase(pr, repo);

    if !result {
        return false;
    }

    let head_ref = &pr.head.ref_field;
    let head = repo.resolve_reference_from_short_name(head_ref).unwrap();

    if is_safe_branch(repo, &head, head_ref) {
        info!("No changes for \"{}\". Not pushing to remote.", pr.title);
        return false;
    }

    push(pr, repo, origin_remote)
}

pub(crate) fn with_revert_to_current_branch<F: FnMut()>(repo: &Repository, mut f: F) {
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

fn init_octocrab(host: &str) -> Octocrab {
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
        .unwrap_or_else(|_| panic!("File {} is missing", filename))
        .parse::<Value>()
        .unwrap_or_else(|_| panic!("Error parsing {}", filename));

    let config_table = config
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing {}", filename));

    let github_table = config_table
        .get(host)
        .unwrap_or_else(|| panic!("{} table missing from {}", host, filename))
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing table {} from {}", host, filename));

    github_table
        .get("oauth")
        .unwrap_or_else(|| panic!("Missing oauth key for {} in {}", host, filename))
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "Expected string for oauth key under {} in {}",
                host, filename
            )
        })
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
