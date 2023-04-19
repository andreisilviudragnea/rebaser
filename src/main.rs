use log::{debug, info, LevelFilter};
use octocrab::models::pulls::PullRequest;

use simple_logger::SimpleLogger;

use crate::git::remote::{GitRemote, GitRemoteOps};
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

    let repo = GitRepository::new();

    let mut primary_remote = repo.get_primary_remote();

    primary_remote.fetch();

    let captures = primary_remote.get_host_owner_repo_name();

    let host = &captures[1];
    let owner = &captures[2];
    let repo_name = &captures[3];

    debug!("{host}:{owner}/{repo_name}");

    let github = GithubClient::new(host);

    let github_repo = github.get_repo(owner, repo_name).await;

    debug!("Github repo: {github_repo:?}");

    repo.fast_forward(github_repo.default_branch.as_ref().unwrap());

    let all_my_open_prs = github.get_all_my_open_prs(owner, repo_name).await;

    rebase_and_push_all_my_open_prs(&repo, primary_remote, all_my_open_prs);
}

fn rebase_and_push_all_my_open_prs(
    repo: &GitRepository,
    mut primary_remote: GitRemote,
    all_my_open_prs: Vec<PullRequest>,
) {
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

            changes_propagated =
                (repo.rebase(pr) && primary_remote.push(pr, repo)) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
