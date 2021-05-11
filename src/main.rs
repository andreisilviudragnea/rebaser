use git2::{Repository, Error};
use std::collections::HashMap;
use github_rs::client::{Executor, Github};
use serde_json::Value;
use github_rs::repos::get::Repos;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let repo = Repository::discover(".")?;

    let origin_remote = repo.find_remote("origin")?;

    println!("Origin remote: {}", origin_remote.url().unwrap());

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
