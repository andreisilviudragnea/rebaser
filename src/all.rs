use octocrab::models::pulls::PullRequest;

use crate::git::remote::{GitRemote, GitRemoteOps};

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &GitRepository,
    remote: &mut GitRemote,
) -> bool {
    let result = repo.rebase(pr);

    if !result {
        return false;
    }

    remote.push(pr, repo)
}
