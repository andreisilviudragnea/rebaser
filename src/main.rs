use git2::Repository;
use log::{error, LevelFilter};
use simple_logger::SimpleLogger;

use crate::all::{get_all_my_safe_prs, rebase_and_push, with_revert_to_current_branch};
use crate::git::fetch;

mod all;
mod git;

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let repo = Repository::discover(".").unwrap();

    let remotes_array = repo.remotes().unwrap();

    let remotes = remotes_array
        .iter()
        .map(|it| it.unwrap())
        .collect::<Vec<&str>>();

    if remotes.len() > 1 {
        error!("Multiple remotes not supported yet.");
        return;
    }

    let mut origin_remote = repo.find_remote(remotes[0]).unwrap();

    fetch(&mut origin_remote).unwrap();

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
