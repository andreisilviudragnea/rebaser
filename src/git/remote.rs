use git2::{Direction, Remote};
use log::debug;

use crate::git::repository::remote_callbacks;
use regex::{Captures, Regex};

pub(crate) trait GitRemoteOps {
    fn get_host_owner_repo_name(&self) -> Captures;
    fn default_branch(&mut self) -> String;
}

pub(crate) struct GitRemote<'repo>(Remote<'repo>);

impl GitRemote<'_> {
    pub(crate) fn new(remote: Remote) -> GitRemote {
        GitRemote(remote)
    }
}

impl GitRemoteOps for GitRemote<'_> {
    fn get_host_owner_repo_name(&self) -> Captures {
        let remote_url = self.0.url().unwrap();
        debug!("remote_url: {remote_url}");

        Regex::new(r".*@(.*):(.*)/(.*).git")
            .unwrap()
            .captures(remote_url)
            .unwrap()
    }

    fn default_branch(&mut self) -> String {
        let mut connection = self
            .0
            .connect_auth(Direction::Push, Some(remote_callbacks()), None)
            .unwrap();
        assert!(connection.connected());
        connection
            .default_branch()
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }
}
