use git2::Repository;
use log::{debug, LevelFilter};
use simple_logger::SimpleLogger;

use git::remote::{GitRemote, GitRemoteOps};
use git::repository::GitRepository;

use crate::git::repository::{GitRepo, RepositoryOps};
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

    let git_repo = GitRepo { repository: &repo };

    let (host, owner, repo_name) = primary_remote.get_host_owner_repo_name();

    let github = GithubClient::new(&host);

    let github_repo = github.get_repo(&owner, &repo_name).await;

    debug!("Github repo: {github_repo:?}");

    repo.fast_forward(github_repo.default_branch.as_ref().unwrap());

    let all_my_safe_prs = git_repo
        .get_all_my_safe_prs(&github, &owner, &repo_name)
        .await;

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
