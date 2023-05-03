use git2::{Remote, Repository};
use log::{debug, info, LevelFilter};
use octocrab::models::pulls::PullRequest;
use regex::{Captures, Regex};
use std::process::Command;

use crate::git::{GitRepository, RepositoryOps};
use simple_logger::SimpleLogger;

use crate::github::{Github, GithubClient};

mod git;
mod github;

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_utc_timestamps()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    fetch_all_remotes();

    let mut repository = Repository::discover(".").unwrap();

    let repo = GitRepository::new(&mut repository);

    let origin = repo.get_origin_remote();

    let captures = get_host_owner_repo_name(&origin);

    let (host, owner, repo_name) = (&captures[1], &captures[2], &captures[3]);

    debug!("{host}:{owner}/{repo_name}");

    let github = GithubClient::new(host);

    let github_repo = github.get_repo(owner, repo_name).await;

    debug!("Github repo: {github_repo:?}");

    repo.fast_forward(github_repo.default_branch.as_ref().unwrap());

    rebase_all_my_open_prs(&repo, github.get_all_my_open_prs(owner, repo_name).await);

    push_all_branches();
}

fn fetch_all_remotes() {
    assert!(Command::new("git")
        .arg("fetch")
        .arg("--all")
        .status()
        .expect("git fetch --all should not fail")
        .success());
}

fn get_host_owner_repo_name<'repo>(remote: &'repo Remote<'repo>) -> Captures<'repo> {
    let remote_url = remote.url().unwrap();
    debug!("remote_url: {remote_url}");

    Regex::new(r".*@(.*):(.*)/(.*).git")
        .unwrap()
        .captures(remote_url)
        .unwrap()
}

fn rebase_all_my_open_prs(repo: &GitRepository, all_my_open_prs: Vec<PullRequest>) {
    repo.with_revert_to_current_branch(|| loop {
        info!("Recursively rebasing...");

        let mut changes_propagated = false;

        all_my_open_prs.iter().for_each(|pr| {
            if !repo.is_safe_pr(pr) {
                info!(
                    "Not rebasing \"{}\" {} <- {} because it is unsafe",
                    pr.title.as_ref().unwrap(),
                    pr.base.ref_field,
                    pr.head.ref_field
                );
                return;
            }

            repo.rebase(pr);

            changes_propagated = repo.needs_rebasing(pr) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}

fn push_all_branches() {
    assert!(Command::new("git")
        .arg("-c")
        .arg("push.default=matching")
        .arg("push")
        .arg("--force-with-lease")
        .status()
        .expect("git -c push.default=matching push --force-with-lease should not fail")
        .success());
}
