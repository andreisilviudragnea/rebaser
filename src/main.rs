use std::collections::HashMap;
use std::env;

use git2::{Cred, Error, FetchOptions, Remote, RemoteCallbacks, Repository};
use octocrab::params::State;
use regex::Regex;
use octocrab::models::pulls::PullRequest;

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
    let repo = &captures[2];
    println!("Remote repo: {}/{}", owner, repo);

    let mut settings = config::Config::default();
    settings.merge(config::Environment::with_prefix("GITHUB")).unwrap();

    let map = settings.try_into::<HashMap<String, String>>().unwrap();
    println!("{:?}", map);

    let octocrab = octocrab::OctocrabBuilder::new()
        .personal_token(map.get("oauth").unwrap().clone())
        .build()
        .unwrap();

    let user = octocrab.current().user().await.unwrap();

    let pull_request_handler = octocrab.pulls(owner, repo);

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

    println!("{}", my_prs.len());

    Ok(())
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
