use std::env;

use git2::BranchType::Local;
use git2::ResetType::Hard;
use git2::{Cred, FetchOptions, PushOptions, Remote, RemoteCallbacks};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use regex::{Captures, Regex};

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) trait GitRemoteOps {
    fn fetch(&mut self);

    fn push(&mut self, pr: &PullRequest, repo: &GitRepository) -> bool;

    fn get_host_owner_repo_name(&self) -> Captures;
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

        self.0
            .fetch(
                &[format!("+refs/heads/*:refs/remotes/{remote_name}/*")],
                Some(&mut fetch_options),
                Some(format!("Fetched from remote {remote_name}").as_str()),
            )
            .unwrap()
    }

    fn push(&mut self, pr: &PullRequest, repo: &GitRepository) -> bool {
        let head = &pr.head.ref_field;

        let local_head_branch = repo.find_branch(head, Local).unwrap();

        let upstream_head_branch = local_head_branch.upstream().unwrap();

        let upstream_head_ref = upstream_head_branch.get();

        let pr_title = pr.title.as_ref().unwrap();

        if local_head_branch.get() == upstream_head_ref {
            info!("No changes for \"{pr_title}\". Not pushing to remote.");
            return false;
        }

        debug!("Pushing changes to remote...");

        let mut remote = repo
            .repository
            .find_remote(
                repo.repository
                    .branch_upstream_name(local_head_branch.name().unwrap().unwrap())
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap();

        let mut options = PushOptions::new();

        options.remote_callbacks(credentials_callback());

        match remote.push(&[format!("+refs/heads/{head}")], Some(&mut options)) {
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

    fn get_host_owner_repo_name(&self) -> Captures {
        let remote_url = self.0.url().unwrap();
        debug!("remote_url: {remote_url}");

        Regex::new(r".*@(.*):(.*)/(.*).git")
            .unwrap()
            .captures(remote_url)
            .unwrap()
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
