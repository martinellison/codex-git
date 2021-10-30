/*! Code for fetch copied from the `git2` crate examples.
 *
 * Copied from https://github.com/rust-lang/git2-rs/blob/master/examples/pull.rs and modified. libgit2 "pull" example - shows how to pull remote data into a local branch.
 *
 * Original written by the libgit2 contributors.
*/
use ansi_term::Colour::*;
use git2::{RemoteCallbacks, Repository};
use log::trace;
use std::str;

// #[derive(StructOpt)]
// struct Args {
//     arg_remote: Option<String>,
//     arg_branch: Option<String>,
// }

/// tracing macro
macro_rules! git_pull_trace {
    () => { trace!() };
    ($($arg:tt)*) => {
        trace!("{} ({}:{})", Purple.on(Cyan).paint(format!($($arg)*)), std::file!(), std::line!());
    };
}
/// performs a `git2` fetch.
pub fn do_fetch<'a>(
    repo: &'a git2::Repository,
    refs: &[&str],
    remote: &'a mut git2::Remote,
    cb: RemoteCallbacks,
) -> Result<git2::AnnotatedCommit<'a>, git2::Error> {
    git_pull_trace!("fetching...");
    //let mut cb = git2::RemoteCallbacks::new();

    // // Print out our transfer progress.
    // cb.transfer_progress(|stats| {
    //     if stats.received_objects() == stats.total_objects() {
    //         print!(
    //             "Resolving deltas {}/{}\r",
    //             stats.indexed_deltas(),
    //             stats.total_deltas()
    //         );
    //     } else if stats.total_objects() > 0 {
    //         print!(
    //             "Received {}/{} objects ({}) in {} bytes\r",
    //             stats.received_objects(),
    //             stats.total_objects(),
    //             stats.indexed_objects(),
    //             stats.received_bytes()
    //         );
    //     }
    //     io::stdout().flush().unwrap();
    //     true
    // });

    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(cb);
    // Always fetch all tags.
    // Perform a download and also update tips
    fo.download_tags(git2::AutotagOption::All);
    git_pull_trace!("Fetching {} for repo", remote.name().unwrap());
    remote.fetch(refs, Some(&mut fo), None)?;

    // If there are local objects (we got a thin pack), then tell the user
    // how many objects we saved from having to cross the network.
    let stats = remote.stats();
    if stats.local_objects() > 0 {
        git_pull_trace!(
            "\rReceived {}/{} objects in {} bytes (used {} local \
             objects)",
            stats.indexed_objects(),
            stats.total_objects(),
            stats.received_bytes(),
            stats.local_objects()
        );
    } else {
        git_pull_trace!(
            "\rReceived {}/{} objects in {} bytes",
            stats.indexed_objects(),
            stats.total_objects(),
            stats.received_bytes()
        );
    }

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let commit = repo.reference_to_annotated_commit(&fetch_head)?;
    git_pull_trace!("fetched {}", commit.refname().unwrap_or("[not valid]"));
    Ok(commit)
}

/// performs a `git2` fast forward
fn fast_forward(
    repo: &Repository,
    lb: &mut git2::Reference,
    rc: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    git_pull_trace!("fast forwarding...");
    let name = match lb.name() {
        Some(s) => s.to_string(),
        None => String::from_utf8_lossy(lb.name_bytes()).to_string(),
    };
    let msg = format!("Fast-Forward: Setting {} to id: {}", name, rc.id());
    git_pull_trace!("{}", msg);
    lb.set_target(rc.id(), &msg)?;
    repo.set_head(&name)?;
    repo.checkout_head(Some(
        git2::build::CheckoutBuilder::default()
            // For some reason the force is required to make the working directory actually get updated
            // I suspect we should be adding some logic to handle dirty working directory states
            // but this is just an example so maybe not.
            .force(),
    ))?;
    Ok(())
}

/** performs a `git2` 'normal' merge (not fast forward). Conficts are detected
but not handled. */
fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    git_pull_trace!("merging normally...");
    let local_tree = repo.find_commit(local.id())?.tree()?;
    let remote_tree = repo.find_commit(remote.id())?.tree()?;
    let ancestor = repo
        .find_commit(repo.merge_base(local.id(), remote.id())?)?
        .tree()?;
    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if idx.has_conflicts() {
        git_pull_trace!("Merge conficts detected...");
        repo.checkout_index(Some(&mut idx), None)?;
        return Ok(());
    }
    let result_tree = repo.find_tree(idx.write_tree_to(repo)?)?;
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = repo.signature()?;
    let local_commit = repo.find_commit(local.id())?;
    let remote_commit = repo.find_commit(remote.id())?;
    // Do our merge commit and set current branch head to that commit.
    let merge_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    )?;
    for e in repo.find_commit(merge_commit)?.tree()?.iter() {
        git_pull_trace!("merge tree has {:?}", &e.name().unwrap_or("[not valid]"));
    }
    // Set working tree to match head.
    repo.checkout_head(None)?;
    Ok(())
}

/// performs a `git2` merge after a fetch.
pub fn do_merge<'a>(
    repo: &'a Repository,
    remote_branch: &str,
    fetch_commit: git2::AnnotatedCommit<'a>,
) -> Result<(), git2::Error> {
    git_pull_trace!("doing merge...");
    // 1. do a merge analysis
    let analysis = repo.merge_analysis(&[&fetch_commit])?;

    // 2. Do the appopriate merge
    if analysis.0.is_fast_forward() {
        git_pull_trace!("Doing a fast forward");
        // do a fast forward
        let refname = format!("refs/heads/{}", remote_branch);
        match repo.find_reference(&refname) {
            Ok(mut r) => {
                fast_forward(repo, &mut r, &fetch_commit)?;
            }
            Err(_) => {
                // The branch doesn't exist so just set the reference to the
                // commit directly. Usually this is because you are pulling
                // into an empty repository.
                git_pull_trace!("no branch, setting head to commit");
                repo.reference(
                    &refname,
                    fetch_commit.id(),
                    true,
                    &format!("Setting {} to {}", remote_branch, fetch_commit.id()),
                )?;
                repo.set_head(&refname)?;
                repo.checkout_head(Some(
                    git2::build::CheckoutBuilder::default()
                        .allow_conflicts(true)
                        .conflict_style_merge(true)
                        .force(),
                ))?;
            }
        };
    } else if analysis.0.is_normal() {
        // do a normal merge
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(&repo, &head_commit, &fetch_commit)?;
    } else {
        git_pull_trace!("Nothing to do...");
    }
    Ok(())
}

// fn run(args: &Args) -> Result<(), git2::Error> {
//     let remote_name = args.arg_remote.as_ref().map(|s| &s[..]).unwrap_or("origin");
//     let remote_branch = args.arg_branch.as_ref().map(|s| &s[..]).unwrap_or("master");
//     let repo = Repository::open(".")?;
//     let mut remote = repo.find_remote(remote_name)?;
//     let fetch_commit = do_fetch(&repo, &[remote_branch], &mut remote)?;
//     do_merge(&repo, &remote_branch, fetch_commit)
// }

// fn main() {
//     let args = Args::from_args();
//     match run(&args) {
//         Ok(()) => {}
//         Err(e) => git_pull_trace!("error: {}", e),
//     }
// }

/*
 * To the extent possible under law, the author(s) have dedicated all copyright and related and neighboring rights to the original software to the public domain worldwide. This software is distributed without any warranty. See <http://creativecommons.org/publicdomain/zero/1.0/>.
 */
