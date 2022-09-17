use git2::Repository;
use log::{debug, info, LevelFilter};
use octocrab::models::pulls::PullRequest;
use simple_logger::SimpleLogger;

use git::remote::{GitRemote, GitRemoteOps};
use git::repository::GitRepository;

use crate::git::repository::RepositoryOps;
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

    let git_repo = Repository::discover(".").unwrap();

    let repo = GitRepository::new(&git_repo);

    let mut primary_remote = repo.get_primary_remote();

    primary_remote.fetch();

    let (host, owner, repo_name) = primary_remote.get_host_owner_repo_name();

    let github = GithubClient::new(&host);

    let github_repo = github.get_repo(&owner, &repo_name).await;

    debug!("Github repo: {github_repo:?}");

    repo.fast_forward(github_repo.default_branch.as_ref().unwrap());

    let all_my_open_prs = github.get_all_my_open_prs(&owner, &repo_name).await;

    let num_of_my_open_prs = all_my_open_prs.len();

    let all_my_safe_prs = all_my_open_prs
        .into_iter()
        .filter(|pr| repo.is_safe_pr(pr))
        .collect::<Vec<PullRequest>>();

    info!(
        "Going to rebase {}/{num_of_my_open_prs} safe pull requests:",
        all_my_safe_prs.len()
    );

    all_my_safe_prs.iter().for_each(|pr| {
        info!(
            "\"{}\" {} <- {}",
            pr.title.as_ref().unwrap(),
            pr.base.ref_field,
            pr.head.ref_field
        );
    });

    repo.with_revert_to_current_branch(|| loop {
        let mut changes_propagated = false;

        all_my_safe_prs.iter().for_each(|pr| {
            changes_propagated =
                (repo.rebase(pr) && primary_remote.push(pr, &repo)) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
