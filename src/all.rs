use git2::ResetType::Hard;
use log::{error, info};
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

    let head = &pr.head.ref_field;

    let head_ref = repo.resolve_reference_from_short_name(head).unwrap();

    let remote_head_ref = repo
        .resolve_reference_from_short_name(&format!("{}/{head}", remote.name()))
        .unwrap();

    let pr_title = pr.title.as_ref().unwrap();

    if head_ref == remote_head_ref {
        info!("No changes for \"{pr_title}\". Not pushing to remote.");
        return false;
    }

    info!("Pushing changes to remote...");

    match remote.push(head) {
        Ok(()) => {
            info!("Successfully pushed changes to remote for \"{pr_title}\"");
            true
        }
        Err(e) => {
            error!("Push to remote failed for \"{pr_title}\": {e}. Resetting...");

            let remote_commit = remote_head_ref.peel_to_commit().unwrap();

            repo.reset(remote_commit.as_object(), Hard, None);

            info!("Successfully reset.");

            false
        }
    }
}
