use git2::Repository;
use log::{info, LevelFilter};
use simple_logger::SimpleLogger;

use git::remote::{GitRemote, GitRemoteOps};
use git::repository::GitRepository;

use crate::all::{get_all_my_safe_prs, rebase_and_push, with_revert_to_current_branch};
use crate::git::repository::GitRepo;

mod all;
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

    info!("Primary remote: {}", remote.name());

    remote.fetch();

    let git_repo = GitRepo {
        repository: &repo,
        primary_remote: &remote,
    };

    let all_my_safe_prs = get_all_my_safe_prs(&git_repo).await;

    with_revert_to_current_branch(&repo, || loop {
        let mut changes_propagated = false;

        all_my_safe_prs.iter().for_each(|pr| {
            changes_propagated = rebase_and_push(pr, &repo, &mut remote) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
