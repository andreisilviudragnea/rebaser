use std::env;

use git2::{Cred, Error, FetchOptions, PushOptions, Remote, RemoteCallbacks};
use log::debug;
use regex::Regex;

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) trait GitRemoteOps {
    fn fetch(&mut self);

    fn push(&mut self, head_ref: &str) -> Result<(), Error>;

    fn name(&self) -> &str;

    fn get_host_owner_repo_name(&self) -> (String, String, String);

    fn url(&self) -> &str;
}

pub(crate) struct GitRemote<'a>(pub Remote<'a>);

impl<'a> GitRemote<'a> {
    pub(crate) fn new(repo: &'a GitRepository) -> GitRemote<'a> {
        GitRemote(repo.get_primary_remote())
    }
}

impl GitRemoteOps for GitRemote<'_> {
    fn fetch(&mut self) {
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(credentials_callback());

        let remote_name = self.0.name().unwrap();

        let refspecs = format!("+refs/heads/*:refs/remotes/{remote_name}/*");
        let reflog_msg = format!("Fetched from remote {remote_name}");

        self.0
            .fetch(
                &[refspecs],
                Some(&mut fetch_options),
                Some(reflog_msg.as_str()),
            )
            .unwrap()
    }

    fn push(&mut self, head_ref: &str) -> Result<(), Error> {
        let mut options = PushOptions::new();

        options.remote_callbacks(credentials_callback());

        self.0
            .push(&[format!("+refs/heads/{head_ref}")], Some(&mut options))
    }

    fn name(&self) -> &str {
        self.0.name().unwrap()
    }

    fn get_host_owner_repo_name(&self) -> (String, String, String) {
        let remote_url = self.0.url().unwrap();
        debug!("remote_url: {remote_url}");

        let regex = Regex::new(r".*@(.*):(.*)/(.*).git").unwrap();

        let captures = regex.captures(remote_url).unwrap();

        let host = &captures[1];
        let owner = &captures[2];
        let repo_name = &captures[3];

        debug!("{host}:{owner}/{repo_name}");

        (host.to_owned(), owner.to_owned(), repo_name.to_owned())
    }

    fn url(&self) -> &str {
        self.0.url().unwrap()
    }
}

fn credentials_callback<'a>() -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_ed25519", env::var("HOME").unwrap())),
            None,
        )
    });
    callbacks
}
