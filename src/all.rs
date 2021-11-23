use git2::Reference;
use git2::ResetType::Hard;
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use regex::Regex;

use crate::git::remote::{GitRemote, GitRemoteOps};
use crate::git::repository::{GitRepository, RepositoryOps};
use crate::github::{Github, GithubClient};

fn is_safe_branch(
    repo: &GitRepository,
    reference: &Reference,
    remote_reference: &Reference,
) -> bool {
    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, reference, remote_reference);

    let reference_name = reference.name().unwrap();
    let remote_reference_reference_name = remote_reference.name().unwrap();

    if number_of_commits_ahead > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits ahead \"{}\"",
            reference_name, number_of_commits_ahead, remote_reference_reference_name
        );
        return false;
    }

    if number_of_commits_behind > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits behind \"{}\"",
            reference_name, number_of_commits_behind, remote_reference_reference_name
        );
        return false;
    }

    true
}

fn compare_refs(repo: &GitRepository, head: &Reference, base: &Reference) -> (usize, usize) {
    let head_commit_name = head.name().unwrap();
    let base_commit_name = base.name().unwrap();

    (
        repo.log_count(base_commit_name, head_commit_name).unwrap(),
        repo.log_count(head_commit_name, base_commit_name).unwrap(),
    )
}

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &GitRepository,
    remote: &mut GitRemote,
) -> bool {
    let head_ref = &pr.head.ref_field;
    let base_ref = &pr.base.ref_field;

    let pr_title = pr.title.as_ref().unwrap();

    info!("Rebasing \"{}\" {} <- {}...", pr_title, base_ref, head_ref);

    let head = repo.resolve_reference_from_short_name(head_ref).unwrap();
    let base = repo.resolve_reference_from_short_name(base_ref).unwrap();

    let result = repo.rebase(&head, &base).unwrap();

    if !result {
        return false;
    }

    let remote_head = repo
        .resolve_reference_from_short_name(&format!("{}/{}", remote.name(), head_ref))
        .unwrap();

    if is_safe_branch(repo, &head, &remote_head) {
        info!("No changes for \"{}\". Not pushing to remote.", pr_title);
        return false;
    }

    info!("Pushing changes to remote...");

    match remote.push(head_ref) {
        Ok(()) => {
            info!("Successfully pushed changes to remote for \"{}\"", pr_title);
            true
        }
        Err(e) => {
            error!(
                "Push to remote failed for \"{}\": {}. Resetting...",
                pr_title, e
            );

            let remote_commit = remote_head.peel_to_commit().unwrap();

            repo.reset(remote_commit.as_object(), Hard, None).unwrap();

            info!("Successfully reset.");

            false
        }
    }
}

pub(crate) fn with_revert_to_current_branch<F: FnMut()>(repo: &GitRepository, mut f: F) {
    let current_head = repo.head().unwrap();

    let name = current_head.name().unwrap();

    debug!("Current HEAD is {}", name);

    f();

    debug!("Current HEAD is {}", repo.head().unwrap().name().unwrap());

    let reference = repo.resolve_reference_from_short_name(name).unwrap();

    repo.switch(&reference).unwrap();

    debug!("Current HEAD is {}", repo.head().unwrap().name().unwrap());
}

fn is_safe_pr(repo: &GitRepository, remote: &GitRemote, pr: &PullRequest) -> bool {
    let base_ref = &pr.base.ref_field;
    let base = match repo.resolve_reference_from_short_name(base_ref) {
        Ok(reference) => reference,
        Err(e) => {
            error!(
                "Error resolving reference from shortname for {}: {}",
                base_ref, e
            );
            return false;
        }
    };

    let remote_name = remote.name();

    let remote_base_ref = &format!("{}/{}", remote_name, base_ref);
    let remote_base = repo
        .resolve_reference_from_short_name(remote_base_ref)
        .unwrap();

    let pr_title = pr.title.as_ref().unwrap();

    if !is_safe_branch(repo, &base, &remote_base) {
        debug!(
            "Pr \"{}\" is not safe because base ref \"{}\" is not safe",
            pr_title, base_ref
        );
        return false;
    }

    let head_ref = &pr.head.ref_field;
    let head = match repo.resolve_reference_from_short_name(head_ref) {
        Ok(reference) => reference,
        Err(e) => {
            error!(
                "Error resolving reference from shortname for {}: {}",
                head_ref, e
            );
            return false;
        }
    };

    let remote_head_ref = &format!("{}/{}", remote_name, head_ref);
    let remote_head = repo
        .resolve_reference_from_short_name(remote_head_ref)
        .unwrap();

    if !is_safe_branch(repo, &head, &remote_head) {
        debug!(
            "Pr \"{}\" is not safe because head ref \"{}\" is not safe",
            pr_title, head_ref
        );
        return false;
    }

    debug!("\"{}\" {} <- {}", pr_title, base_ref, head_ref);

    let (number_of_commits_ahead, number_of_commits_behind) = compare_refs(repo, &head, &base);

    debug!(
        "\"{}\" is {} commits ahead, {} commits behind \"{}\"",
        head_ref, number_of_commits_ahead, number_of_commits_behind, base_ref
    );

    true
}

fn get_host_owner_repo_name(remote: &GitRemote) -> (String, String, String) {
    let remote_url = remote.url();
    debug!("remote_url: {}", remote_url);

    let regex = Regex::new(r".*@(.*):(.*)/(.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    let host = &captures[1];
    let owner = &captures[2];
    let repo_name = &captures[3];

    debug!("{}:{}/{}", host, owner, repo_name);

    (host.to_owned(), owner.to_owned(), repo_name.to_owned())
}

pub(crate) async fn get_all_my_safe_prs(
    repo: &GitRepository,
    remote: &GitRemote<'_>,
) -> Vec<PullRequest> {
    let (host, owner, repo_name) = get_host_owner_repo_name(remote);

    let github = GithubClient::new(&host);

    let github_repo = github.get_repo(&owner, &repo_name).await;

    debug!("Github repo: {:?}", github_repo);

    repo.fast_forward(remote, github_repo.default_branch.as_ref().unwrap())
        .unwrap();

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
        "Going to rebase {}/{} safe pull requests:",
        my_safe_prs.len(),
        num_of_my_open_prs
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
