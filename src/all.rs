use git2::ResetType::Hard;
use git2::{Reference, Remote, Repository};
use log::{debug, error, info};
use octocrab::models::pulls::PullRequest;
use regex::Regex;

use crate::git::{fast_forward, log_count, push, rebase, switch};
use crate::github::{Github, GithubClient};

fn is_safe_branch(repo: &Repository, reference: &Reference, origin_reference: &Reference) -> bool {
    let (number_of_commits_ahead, number_of_commits_behind) =
        compare_refs(repo, reference, origin_reference);

    let reference_name = reference.name().unwrap();
    let origin_reference_name = origin_reference.name().unwrap();

    if number_of_commits_ahead > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits ahead \"{}\"",
            reference_name, number_of_commits_ahead, origin_reference_name
        );
        return false;
    }

    if number_of_commits_behind > 0 {
        debug!(
            "Branch \"{}\" is unsafe because it is {} commits behind \"{}\"",
            reference_name, number_of_commits_behind, origin_reference_name
        );
        return false;
    }

    true
}

fn compare_refs(repo: &Repository, head: &Reference, base: &Reference) -> (usize, usize) {
    let head_commit_name = head.name().unwrap();
    let base_commit_name = base.name().unwrap();

    (
        log_count(repo, base_commit_name, head_commit_name).unwrap(),
        log_count(repo, head_commit_name, base_commit_name).unwrap(),
    )
}

pub(crate) fn rebase_and_push(
    pr: &PullRequest,
    repo: &Repository,
    origin_remote: &mut Remote,
) -> bool {
    let head_ref = &pr.head.ref_field;
    let base_ref = &pr.base.ref_field;

    info!("Rebasing \"{}\" {} <- {}...", pr.title, base_ref, head_ref);

    let head = repo.resolve_reference_from_short_name(head_ref).unwrap();
    let base = repo.resolve_reference_from_short_name(base_ref).unwrap();

    let result = rebase(repo, &head, &base).unwrap();

    if !result {
        return false;
    }

    let origin_head = repo
        .resolve_reference_from_short_name(&format!("origin/{}", head_ref))
        .unwrap();

    if is_safe_branch(repo, &head, &origin_head) {
        info!("No changes for \"{}\". Not pushing to remote.", pr.title);
        return false;
    }

    info!("Pushing changes to remote...");

    match push(origin_remote, head_ref) {
        Ok(()) => {
            info!("Successfully pushed changes to remote for \"{}\"", pr.title);
            true
        }
        Err(e) => {
            error!(
                "Push to remote failed for \"{}\": {}. Resetting...",
                pr.title, e
            );

            let origin_commit = origin_head.peel_to_commit().unwrap();

            repo.reset(origin_commit.as_object(), Hard, None).unwrap();

            info!("Successfully reset.");

            false
        }
    }
}

pub(crate) fn with_revert_to_current_branch<F: FnMut()>(repo: &Repository, mut f: F) {
    let current_head = repo.head().unwrap();
    debug!("Current HEAD is {}", current_head.name().unwrap());

    f();

    let head = repo.head().unwrap();
    debug!("Current HEAD is {}", head.name().unwrap());

    switch(repo, &current_head).unwrap();

    let head = repo.head().unwrap();
    debug!("Current HEAD is {}", head.name().unwrap());
}

fn is_safe_pr(repo: &Repository, pr: &PullRequest) -> bool {
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

    let origin_base_ref = &format!("origin/{}", base_ref);
    let origin_base = repo
        .resolve_reference_from_short_name(origin_base_ref)
        .unwrap();

    if !is_safe_branch(repo, &base, &origin_base) {
        debug!(
            "Pr \"{}\" is not safe because base ref \"{}\" is not safe",
            pr.title, base_ref
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

    let origin_head_ref = &format!("origin/{}", head_ref);
    let origin_head = repo
        .resolve_reference_from_short_name(origin_head_ref)
        .unwrap();

    if !is_safe_branch(repo, &head, &origin_head) {
        debug!(
            "Pr \"{}\" is not safe because head ref \"{}\" is not safe",
            pr.title, head_ref
        );
        return false;
    }

    debug!("\"{}\" {} <- {}", pr.title, base_ref, head_ref);

    let (number_of_commits_ahead, number_of_commits_behind) = compare_refs(repo, &head, &base);

    debug!(
        "\"{}\" is {} commits ahead, {} commits behind \"{}\"",
        head_ref, number_of_commits_ahead, number_of_commits_behind, base_ref
    );

    true
}

fn get_host_owner_repo_name(origin_remote: &Remote) -> (String, String, String) {
    let remote_url = origin_remote.url().unwrap();
    debug!("Origin remote: {}", remote_url);

    let regex = Regex::new(r".*@(.*):(.*)/(.*).git").unwrap();

    let captures = regex.captures(remote_url).unwrap();

    let host = &captures[1];
    let owner = &captures[2];
    let repo_name = &captures[3];

    debug!("{}:{}/{}", host, owner, repo_name);

    (host.to_owned(), owner.to_owned(), repo_name.to_owned())
}

pub(crate) async fn get_all_my_safe_prs(
    repo: &Repository,
    origin_remote: &Remote<'_>,
) -> Vec<PullRequest> {
    let (host, owner, repo_name) = get_host_owner_repo_name(origin_remote);

    let github = GithubClient::new(&host);

    let repository = github.get_repo(&owner, &repo_name).await;

    debug!("repo: {:?}", repository);

    with_revert_to_current_branch(repo, || {
        fast_forward(repo, repository.default_branch.as_ref().unwrap()).unwrap();
    });

    let all_prs = github.get_all_open_prs(&owner, &repo_name).await;

    let user = github.get_current_user().await;

    let my_open_prs = all_prs
        .into_iter()
        .filter(|pr| pr.user == user)
        .collect::<Vec<PullRequest>>();

    let num_of_my_open_prs = my_open_prs.len();

    let my_safe_prs = my_open_prs
        .into_iter()
        .filter(|pr| is_safe_pr(repo, pr))
        .collect::<Vec<PullRequest>>();

    info!(
        "Going to rebase {}/{} safe pull requests:",
        my_safe_prs.len(),
        num_of_my_open_prs
    );

    my_safe_prs.iter().for_each(|pr| {
        info!(
            "\"{}\" {} <- {}",
            pr.title, pr.base.ref_field, pr.head.ref_field
        );
    });

    my_safe_prs
}
