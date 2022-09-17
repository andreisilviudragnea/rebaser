use git2::ResetType::Hard;
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;

use crate::git::remote::{GitRemote, GitRemoteOps};

use crate::git::repository::{GitRepository, RepositoryOps};

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &GitRepository,
    remote: &mut GitRemote,
) -> bool {
    let head = &pr.head.ref_field;
    let base = &pr.base.ref_field;

    let pr_title = pr.title.as_ref().unwrap();

    info!("Rebasing \"{pr_title}\" {base} <- {head}...");

    let result = repo.rebase(head, base);

    if !result {
        return false;
    }

    let head_ref = repo.resolve_reference_from_short_name(head).unwrap();

    let remote_head_ref = repo
        .resolve_reference_from_short_name(&format!("{}/{head}", remote.name()))
        .unwrap();

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

pub(crate) fn with_revert_to_current_branch<F: FnMut()>(repo: &GitRepository, mut f: F) {
    let current_head = repo.head();

    let name = current_head.name().unwrap();

    debug!("Current HEAD is {name}");

    f();

    debug!("Current HEAD is {}", repo.head().name().unwrap());

    let reference = repo.resolve_reference_from_short_name(name).unwrap();

    repo.switch(&reference);

    debug!("Current HEAD is {}", repo.head().name().unwrap());
}
