use git2::Repository;
use log::{debug, info, LevelFilter};
use octocrab::models::pulls::PullRequest;
use std::process::Command;

use simple_logger::SimpleLogger;

use crate::git::remote::GitRemoteOps;
use crate::git::repository::{GitRepository, RepositoryOps};
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

    let mut origin = repo.get_origin_remote();

    let captures = origin.get_host_owner_repo_name();

    let (host, owner, repo_name) = (&captures[1], &captures[2], &captures[3]);

    debug!("{host}:{owner}/{repo_name}");

    let all_my_open_prs = GithubClient::new(host)
        .get_all_my_open_prs(owner, repo_name)
        .await;

    repo.fast_forward(origin.default_branch());

    rebase_and_push_all_my_open_prs(&repo, all_my_open_prs);
}

fn fetch_all_remotes() {
    assert!(Command::new("git")
        .arg("fetch")
        .arg("--all")
        .status()
        .expect("git fetch --all should not fail")
        .success());
}

fn rebase_and_push_all_my_open_prs(repo: &GitRepository, all_my_open_prs: Vec<PullRequest>) {
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

            changes_propagated = (repo.rebase(pr) && repo.push(pr)) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
