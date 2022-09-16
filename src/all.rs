use git2::Reference;
use git2::ResetType::Hard;
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;

use crate::git::remote::{GitRemote, GitRemoteOps};
use crate::git::repository::{GitRepository, RepositoryOps};
use crate::github::{Github, GithubClient};

fn compare_refs(repo: &GitRepository, head: &Reference, base: &Reference) -> (usize, usize) {
    let head_commit_name = head.name().unwrap();
    let base_commit_name = base.name().unwrap();

    (
        repo.log_count(base_commit_name, head_commit_name),
        repo.log_count(head_commit_name, base_commit_name),
    )
}

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

fn is_safe_pr(repo: &GitRepository, remote: &GitRemote, pr: &PullRequest) -> bool {
    let base = &pr.base.ref_field;

    let base_ref = match repo.resolve_reference_from_short_name(base) {
        Ok(reference) => reference,
        Err(e) => {
            error!("Error resolving reference from shortname for {base}: {e}");
            return false;
        }
    };

    let remote_name = remote.name();

    let remote_base_ref = repo
        .resolve_reference_from_short_name(&format!("{}/{base}", remote_name))
        .unwrap();

    let pr_title = pr.title.as_ref().unwrap();

    if base_ref != remote_base_ref {
        debug!("Pr \"{pr_title}\" is not safe because base ref \"{base}\" is not safe");
        return false;
    }

    let head = &pr.head.ref_field;

    let head_ref = match repo.resolve_reference_from_short_name(head) {
        Ok(reference) => reference,
        Err(e) => {
            error!("Error resolving reference from shortname for {head}: {e}");
            return false;
        }
    };

    let remote_head_ref = repo
        .resolve_reference_from_short_name(&format!("{}/{}", remote_name, head))
        .unwrap();

    if head_ref != remote_head_ref {
        debug!("Pr \"{pr_title}\" is not safe because head ref \"{head}\" is not safe");
        return false;
    }

    debug!("\"{pr_title}\" {base} <- {head}");

    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, &head_ref, &base_ref);

    debug!(
        "\"{head}\" is {number_of_commits_ahead} commits ahead, {number_of_commits_behind} commits behind \"{base}\""
    );

    true
}

pub(crate) async fn get_all_my_safe_prs(
    repo: &GitRepository,
    remote: &GitRemote<'_>,
) -> Vec<PullRequest> {
    let (host, owner, repo_name) = remote.get_host_owner_repo_name();

    let github = GithubClient::new(&host);

    let github_repo = github.get_repo(&owner, &repo_name).await;

    debug!("Github repo: {github_repo:?}");

    repo.fast_forward(remote, github_repo.default_branch.as_ref().unwrap());

    let all_prs = github.get_all_open_prs(&owner, &repo_name).await;

    let user = github.get_current_user().await;

    let my_open_prs = all_prs
        .into_iter()
        .filter(|pr| **pr.user.as_ref().unwrap() == user)
        .collect::<Vec<PullRequest>>();

    let num_of_my_open_prs = my_open_prs.len();

    let my_safe_prs = my_open_prs
        .into_iter()
        .filter(|pr| is_safe_pr(repo, remote, pr))
        .collect::<Vec<PullRequest>>();

    info!(
        "Going to rebase {}/{num_of_my_open_prs} safe pull requests:",
        my_safe_prs.len()
    );

    my_safe_prs.iter().for_each(|pr| {
        info!(
            "\"{}\" {} <- {}",
            pr.title.as_ref().unwrap(),
            pr.base.ref_field,
            pr.head.ref_field
        );
    });

    my_safe_prs
}
