use std::env::var;
use std::fs;

use log::debug;
use octocrab::models::pulls::PullRequest;
use octocrab::models::Repository;
use octocrab::params::State;
use octocrab::{Octocrab, OctocrabBuilder};
use toml::Value;

pub(crate) trait Github {
    async fn get_repo(&self, owner: &str, repo: &str) -> Repository;
    async fn get_all_my_open_prs(&self, owner: &str, repo: &str) -> Vec<PullRequest>;
}

pub(crate) struct GithubClient {
    octocrab: Octocrab,
}

impl GithubClient {
    pub(crate) fn new(host: &str) -> GithubClient {
        GithubClient {
            octocrab: init_octocrab(host),
        }
    }
}

fn init_octocrab(host: &str) -> Octocrab {
    OctocrabBuilder::new()
        .base_uri(if host == "github.com" {
            "https://api.github.com".to_string()
        } else {
            format!("https://{host}/api/v3")
        })
        .unwrap()
        .personal_token(get_oauth_token(host))
        .build()
        .unwrap()
}

fn get_oauth_token(host: &str) -> String {
    let filename = format!("{}/.github", var("HOME").unwrap());

    let config = fs::read_to_string(&filename)
        .unwrap_or_else(|_| panic!("File {filename} is missing"))
        .parse::<Value>()
        .unwrap_or_else(|_| panic!("Error parsing {filename}"));

    let config_table = config
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing {filename}"));

    let github_table = config_table
        .get(host)
        .unwrap_or_else(|| panic!("{host} table missing from {filename}"))
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing table {host} from {filename}"));

    github_table
        .get("oauth")
        .unwrap_or_else(|| panic!("Missing oauth key for {host} in {filename}"))
        .as_str()
        .unwrap_or_else(|| panic!("Expected string for oauth key under {host} in {filename}"))
        .to_owned()
}

impl Github for GithubClient {
    async fn get_repo(&self, owner: &str, repo: &str) -> Repository {
        self.octocrab
            .get(format!("/repos/{owner}/{repo}"), None::<&()>)
            .await
            .unwrap()
    }

    async fn get_all_my_open_prs(&self, owner: &str, repo: &str) -> Vec<PullRequest> {
        let mut page = self
            .octocrab
            .pulls(owner, repo)
            .list()
            .state(State::Open)
            .send()
            .await
            .unwrap();

        debug!("page {page:?}");

        let mut all_prs = page.items;

        while let Some(url) = page.next {
            page = self.octocrab.get_page(&Some(url)).await.unwrap().unwrap();

            debug!("page {page:?}");

            all_prs.append(&mut page.items);
        }

        let current_user_id = self.octocrab.current().user().await.unwrap().id;

        all_prs
            .into_iter()
            .filter(|pr| pr.user.as_ref().unwrap().id == current_user_id)
            .collect()
    }
}
