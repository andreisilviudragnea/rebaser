use git2::Repository;

use crate::git::{fetch, get_all_my_safe_prs, rebase_and_push};
use url::Url;

mod git;

#[tokio::main]
async fn main() {
    let url = Url::parse("https://git.corp.adobe.com/api/v3").unwrap();
    let result = Url::options()
        .base_url(Some(&url))
        .parse("repos/IMS/ims/pulls");

    let repo = Repository::discover(".").unwrap();

    let mut origin_remote = repo.find_remote("origin").unwrap();

    fetch(&mut origin_remote);

    let all_my_safe_prs = get_all_my_safe_prs(&repo, &origin_remote).await;

    loop {
        let mut changes_propagated = false;

        all_my_safe_prs.iter().for_each(|pr| {
            changes_propagated =
                rebase_and_push(pr, &repo, &mut origin_remote) || changes_propagated;
        });

        if !changes_propagated {
            break;
        }
    }
}
