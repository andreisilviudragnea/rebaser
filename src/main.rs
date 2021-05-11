use git2::{Repository, Error, FetchOptions, RemoteCallbacks, Cred, Remote};
use std::collections::HashMap;
use github_rs::client::{Executor, Github};
use serde_json::Value;
use std::env;
use regex::Regex;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let repo = Repository::discover(".")?;

    let mut origin_remote = repo.find_remote("origin")?;

    fetch(&mut origin_remote);

    let remote_url = origin_remote.url().unwrap();
    println!("Origin remote: {}", remote_url);

    let regex = Regex::new(r".*@.*:(.*/.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    println!("Remote repo: {}", &captures[1]);

    let mut settings = config::Config::default();
    settings.merge(config::Environment::with_prefix("GITHUB")).unwrap();

    let map = settings.try_into::<HashMap<String, String>>().unwrap();
    println!("{:?}", map);

    let octocrab = octocrab::OctocrabBuilder::new()
        .personal_token(map.get("oauth").unwrap().clone())
        .build()
        .unwrap();

    println!("{:?}", octocrab.current().user().await.unwrap());

    let client = Github::new(map.get("oauth").unwrap()).unwrap();
    let me = client.get().user().repos().execute::<Value>();

    if let (_, _, Some(value)) = me.unwrap() {
        if let Value::Array(ref values) = value {
            println!("{:?}", values.len());
        }
        println!("{:?}", value);
    }

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
