use std::env;

use git2::BranchType::Local;
use git2::ResetType::Hard;
use git2::{Cred, FetchOptions, PushOptions, Remote, RemoteCallbacks};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use regex::Regex;

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) trait GitRemoteOps {
    fn fetch(&mut self);

    fn push(&mut self, pr: &PullRequest, repo: &GitRepository) -> bool;

    fn get_host_owner_repo_name(&self) -> (String, String, String);
}

pub(crate) struct GitRemote<'repo>(Remote<'repo>);

impl GitRemote<'_> {
    pub(crate) fn new(remote: Remote) -> GitRemote {
        GitRemote(remote)
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

    fn push(&mut self, pr: &PullRequest, repo: &GitRepository) -> bool {
        let head = &pr.head.ref_field;

        let local_head_branch = repo.repository.find_branch(head, Local).unwrap();

        let upstream_head_branch = local_head_branch.upstream().unwrap();

        let upstream_head_ref = upstream_head_branch.get();

        let pr_title = pr.title.as_ref().unwrap();

        if local_head_branch.get() == upstream_head_ref {
            info!("No changes for \"{pr_title}\". Not pushing to remote.");
            return false;
        }

        info!("Pushing changes to remote...");

        let mut options = PushOptions::new();

        options.remote_callbacks(credentials_callback());

        match self
            .0
            .push(&[format!("+refs/heads/{head}")], Some(&mut options))
        {
            Ok(()) => {
                info!("Successfully pushed changes to remote for \"{pr_title}\"");
                true
            }
            Err(e) => {
                error!("Push to remote failed for \"{pr_title}\": {e}. Resetting...");

                let upstream_commit = upstream_head_ref.peel_to_commit().unwrap();

                repo.reset(upstream_commit.as_object(), Hard, None);

                info!("Successfully reset.");

                false
            }
        }
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
