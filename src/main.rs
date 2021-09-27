use git2::Repository;
use log::{info, LevelFilter};
use simple_logger::SimpleLogger;

use crate::all::{
    get_all_my_safe_prs, get_primary_remote, rebase_and_push, with_revert_to_current_branch,
};
use crate::git::fetch;

mod all;
mod git;
mod github;

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let repo = Repository::discover(".").unwrap();

    let mut remote = get_primary_remote(&repo).unwrap();

    info!("Primary remote: {}", remote.name().unwrap());

    fetch(&mut remote).unwrap();

    let all_my_safe_prs = get_all_my_safe_prs(&repo, &remote).await;

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
