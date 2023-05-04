use git2::{Remote, Repository};
use log::{debug, info, LevelFilter};
use octocrab::models::pulls::PullRequest;
use regex::{Captures, Regex};
use std::collections::HashMap;
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

    let default_branch = github_repo.default_branch.as_ref().unwrap();

    repo.fast_forward(default_branch);

    let all_my_safe_open_prs = github
        .get_all_my_open_prs(owner, repo_name)
        .await
        .into_iter()
        .filter(|pr| {
            if !repo.is_safe_pr(pr) {
                info!(
                    "Not rebasing \"{}\" {} <- {} because it is unsafe",
                    pr.title.as_ref().unwrap(),
                    pr.base.ref_field,
                    pr.head.ref_field
                );
                return false;
            }
            true
        })
        .collect();

    let pr_graph = build_pr_graph(all_my_safe_open_prs);

    rebase_recursively(&repo, &pr_graph, default_branch);

    push_all_branches();
}

fn build_pr_graph(all_my_safe_open_prs: Vec<PullRequest>) -> HashMap<String, Vec<PullRequest>> {
    let mut result: HashMap<String, Vec<PullRequest>> = HashMap::new();

    for pr in all_my_safe_open_prs {
        result
            .entry(pr.base.ref_field.clone())
            .or_default()
            .push(pr);
    }

    result
}

fn rebase_recursively(
    repo: &GitRepository,
    pr_graph: &HashMap<String, Vec<PullRequest>>,
    base: &str,
) {
    let prs = match pr_graph.get(base) {
        None => return,
        Some(prs) => prs,
    };

    for pr in prs {
        repo.rebase(pr);
        rebase_recursively(repo, pr_graph, &pr.head.ref_field);
    }
}

fn fetch_all_remotes() {
    assert!(Command::new("git")
        .arg("fetch")
        .arg("--all")
        .status()
        .expect("git fetch --all should not fail")
        .success());
}

fn get_host_owner_repo_name<'a>(remote: &'a Remote<'_>) -> Captures<'a> {
    let remote_url = remote.url().unwrap();
    debug!("remote_url: {remote_url}");

    Regex::new(r".*@(.*):(.*)/(.*).git")
        .unwrap()
        .captures(remote_url)
        .unwrap()
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
