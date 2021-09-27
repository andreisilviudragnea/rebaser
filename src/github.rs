use async_trait::async_trait;
use octocrab::models::pulls::PullRequest;
use octocrab::models::{Repository, User};
use octocrab::params::State;
use octocrab::{Octocrab, OctocrabBuilder};
use std::env::var;
use std::fs;
use toml::Value;

#[async_trait]
pub(crate) trait Github {
    async fn get_repo(&self, owner: &str, repo_name: &str) -> Repository;
    async fn get_all_open_prs(&self, owner: &str, repo_name: &str) -> Vec<PullRequest>;
    async fn get_current_user(&self) -> User;
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
    let oauth_token = get_oauth_token(host);

    OctocrabBuilder::new()
        .base_url(if host == "github.com" {
            "https://api.github.com/".to_string()
        } else {
            format!("https://{}/api/v3/", host)
        })
        .unwrap()
        .personal_token(oauth_token)
        .build()
        .unwrap()
}

fn get_oauth_token(host: &str) -> String {
    let filename = format!("{}/.github", var("HOME").unwrap());

    let config = fs::read_to_string(&filename)
        .unwrap_or_else(|_| panic!("File {} is missing", filename))
        .parse::<Value>()
        .unwrap_or_else(|_| panic!("Error parsing {}", filename));

    let config_table = config
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing {}", filename));

    let github_table = config_table
        .get(host)
        .unwrap_or_else(|| panic!("{} table missing from {}", host, filename))
        .as_table()
        .unwrap_or_else(|| panic!("Error parsing table {} from {}", host, filename));

    github_table
        .get("oauth")
        .unwrap_or_else(|| panic!("Missing oauth key for {} in {}", host, filename))
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "Expected string for oauth key under {} in {}",
                host, filename
            )
        })
        .to_owned()
}

#[async_trait]
impl Github for GithubClient {
    async fn get_repo(&self, owner: &str, repo_name: &str) -> Repository {
        self.octocrab
            .get(format!("/repos/{}/{}", owner, repo_name), None::<&()>)
            .await
            .unwrap()
    }

    async fn get_all_open_prs(&self, owner: &str, repo_name: &str) -> Vec<PullRequest> {
        let pull_request_handler = self.octocrab.pulls(owner, repo_name);

        let mut page = pull_request_handler
            .list()
            .state(State::Open)
            .send()
            .await
            .unwrap();

        let mut all_prs = page.items.into_iter().collect::<Vec<PullRequest>>();

        while let Some(url) = &page.next {
            page = self
                .octocrab
                .get_page(&Some(url.to_owned()))
                .await
                .unwrap()
                .unwrap();

            for item in page.items {
                all_prs.push(item)
            }
        }

        all_prs
    }

    async fn get_current_user(&self) -> User {
        self.octocrab.current().user().await.unwrap()
    }
}
