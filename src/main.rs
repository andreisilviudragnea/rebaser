use std::collections::HashMap;

use git2::Repository;
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;

use crate::git::{describe, fetch, get_owner_repo_name, is_safe_pr, rebase_and_push};
use octocrab::{Octocrab, Page};

mod git;

#[tokio::main]
async fn main() {
    let repo = Repository::discover(".").unwrap();

    let mut origin_remote = repo.find_remote("origin").unwrap();

    fetch(&mut origin_remote);

    let (owner, repo_name) = get_owner_repo_name(&origin_remote);

    let mut settings = config::Config::default();
    settings
        .merge(config::Environment::with_prefix("GITHUB"))
        .unwrap();

    let map = settings.try_into::<HashMap<String, String>>().unwrap();
    println!("{:?}", map);

    let octocrab = octocrab::OctocrabBuilder::new()
        .personal_token(map.get("oauth").unwrap().clone())
        .build()
        .unwrap();

    let user = octocrab.current().user().await.unwrap();

    let pull_request_handler = octocrab.pulls(owner, repo_name);

    let mut page = pull_request_handler
        .list()
        .state(State::Open)
        .per_page(1)
        .send()
        .await
        .unwrap();

    let mut all_my_open_prs = page
        .items
        .into_iter()
        .filter(|it| it.user == user)
        .collect::<Vec<PullRequest>>();

    loop {
        match &page.next {
            None => break,
            Some(url) => {
                page = octocrab
                    .get_page(&Some(url.to_owned()))
                    .await
                    .unwrap()
                    .unwrap();

                let my_open_prs = page.items.into_iter().filter(|it| it.user == user);

                for item in my_open_prs {
                    all_my_open_prs.push(item)
                }
            }
        }
    }

    all_my_open_prs.iter().for_each(|pr| describe(pr, &repo));

    let safe_prs = all_my_open_prs
        .iter()
        .filter(|pr| is_safe_pr(&repo, pr))
        .collect::<Vec<&PullRequest>>();

    println!();

    println!(
        "Going to rebase {}/{} safe pull requests:",
        safe_prs.len(),
        all_my_open_prs.len()
    );

    safe_prs.iter().for_each(|pr| {
        println!(
            "\"{}\" {} <- {}",
            pr.title, pr.base.ref_field, pr.head.ref_field
        );
    });

    println!();

    loop {
        let mut changes_propagated = false;

        safe_prs.iter().for_each(|pr| {
            changes_propagated =
                rebase_and_push(pr, &repo, &mut origin_remote) || changes_propagated;
            println!()
        });

        if !changes_propagated {
            break;
        }
    }
}
