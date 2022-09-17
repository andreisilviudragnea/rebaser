use git2::Repository;
use log::LevelFilter;
use simple_logger::SimpleLogger;

use git::remote::{GitRemote, GitRemoteOps};
use git::repository::GitRepository;

use crate::git::repository::{GitRepo, RepositoryOps};

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

    let mut remote = GitRemote::new(&repo);

    remote.fetch();

    let git_repo = GitRepo {
        repository: &repo,
        primary_remote: &remote,
    };

    let all_my_safe_prs = git_repo.get_all_my_safe_prs().await;

    repo.with_revert_to_current_branch(|| loop {
        let mut changes_propagated = false;

        all_my_safe_prs.iter().for_each(|pr| {
            changes_propagated = (repo.rebase(pr) && remote.push(pr, &repo)) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
