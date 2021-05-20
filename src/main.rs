use std::collections::HashMap;
use std::env;

use git2::{Cred, Error, FetchOptions, Remote, RemoteCallbacks, Repository};
use octocrab::models::pulls::PullRequest;
use octocrab::params::State;
use regex::Regex;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let repo = Repository::discover(".")?;

    let mut origin_remote = repo.find_remote("origin")?;

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

    let my_prs = page
        .items
        .into_iter()
        .filter(|it| it.user == user)
        .collect::<Vec<PullRequest>>();

    my_prs.iter().for_each(|pr| describe(pr, &repo));

    Ok(())
}

fn describe(pr: &PullRequest, repo: &Repository) {
    let head_ref = &pr.head.ref_field;
    let base_ref = &pr.base.ref_field;

    println!("\"{}\" {} <- {}", pr.title, head_ref, base_ref);

    let head_commit = repo.resolve_reference_from_short_name(head_ref).unwrap();
    let base_commit = repo.resolve_reference_from_short_name(base_ref).unwrap();

    let head_commit_name = head_commit.name().unwrap();
    let base_commit_name = base_commit.name().unwrap();

    let mut revwalk = repo.revwalk().unwrap();
    revwalk.hide_ref(base_commit_name).unwrap();
    revwalk.push_ref(head_commit_name).unwrap();

    let number_of_commits_ahead = revwalk.into_iter().count();

    let mut revwalk = repo.revwalk().unwrap();
    revwalk.hide_ref(head_commit_name).unwrap();
    revwalk.push_ref(base_commit_name).unwrap();

    let number_of_commits_behind = revwalk.into_iter().count();

    println!(
        "\"{}\" is {} commits ahead, {} commits behind \"{}\"",
        head_ref, number_of_commits_ahead, number_of_commits_behind, base_ref
    );

    println!();
}

fn fetch(origin_remote: &mut Remote) {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_rsa", env::var("HOME").unwrap())),
            None,
        )
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    origin_remote
        .fetch(
            &["+refs/heads/*:refs/remotes/origin/*"],
            Some(&mut fetch_options),
            None,
        )
        .unwrap();
}
