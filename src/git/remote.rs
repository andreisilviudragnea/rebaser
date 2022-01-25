use std::env;

use git2::{Cred, Error, FetchOptions, PushOptions, Remote, RemoteCallbacks};

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) trait GitRemoteOps {
    fn fetch(&mut self);

    fn push(&mut self, head_ref: &str) -> Result<(), Error>;

    fn name(&self) -> &str;

    fn url(&self) -> &str;
}

pub(crate) struct GitRemote<'a>(Remote<'a>);

impl<'a> GitRemote<'a> {
    pub(crate) fn new(repo: &'a GitRepository) -> GitRemote<'a> {
        GitRemote(repo.get_primary_remote())
    }
}

impl GitRemoteOps for GitRemote<'_> {
    fn fetch(&mut self) {
        let callbacks = credentials_callback();

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

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
            std::path::Path::new(&format!("{}/.ssh/id_rsa", env::var("HOME").unwrap())),
            None,
        )
    });
    callbacks
}
