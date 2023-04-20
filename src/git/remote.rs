use git2::Remote;
use log::debug;

use regex::{Captures, Regex};

pub(crate) trait GitRemoteOps {
    fn get_host_owner_repo_name(&self) -> Captures;
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
}
