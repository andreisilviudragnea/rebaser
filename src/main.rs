use git2::Repository;

use crate::git::{fetch, get_all_my_safe_prs, rebase_and_push, with_revert_to_current_branch};
use log::LevelFilter;
use simple_logger::SimpleLogger;

mod git;

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Debug)
        .init()
        .unwrap();

    let repo = Repository::discover(".").unwrap();

    let mut origin_remote = repo.find_remote("origin").unwrap();

    fetch(&mut origin_remote);

    let all_my_safe_prs = get_all_my_safe_prs(&repo, &origin_remote).await;

    with_revert_to_current_branch(&repo, || loop {
        let mut changes_propagated = false;

        all_my_safe_prs.iter().for_each(|pr| {
            changes_propagated =
                rebase_and_push(pr, &repo, &mut origin_remote) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    });
}
