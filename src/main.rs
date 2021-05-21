use std::collections::HashMap;

use git2::Repository;
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;
use regex::Regex;

use crate::git::{describe, fetch, is_safe_pr, rebase_and_push};

mod git;

#[tokio::main]
async fn main() {
    let repo = Repository::discover(".").unwrap();

    let mut origin_remote = repo.find_remote("origin").unwrap();

    fetch(&mut origin_remote);

    let remote_url = origin_remote.url().unwrap();
    println!("Origin remote: {}", remote_url);

    let regex = Regex::new(r".*@.*:(.*)/(.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    let owner = &captures[1];
    let repo_name = &captures[2];
    println!("Remote repo: {}/{}", owner, repo_name);

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

    let page = pull_request_handler
        .list()
        .state(State::Open)
        .send()
        .await
        .unwrap();

    let my_open_prs = page
        .items
        .into_iter()
        .filter(|it| it.user == user)
        .collect::<Vec<PullRequest>>();

    my_open_prs.iter().for_each(|pr| describe(pr, &repo));

    let safe_prs = my_open_prs
        .iter()
        .filter(|pr| is_safe_pr(&repo, pr))
        .collect::<Vec<&PullRequest>>();

    println!();

    println!(
        "Going to rebase {}/{} safe pull requests:",
        safe_prs.len(),
        my_open_prs.len()
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
