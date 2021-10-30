/*! codex-git is a simplified wrapper for [git2]. Most of the code is in this file. */

//#![feature(backtrace)]
use ansi_term::Colour::*;
use anyhow::anyhow;
use anyhow::Context;
use getset::{CopyGetters, Getters, Setters};
use git2::IndexAddOption;
use git2::{
    build::RepoBuilder, Commit, Cred, CredentialType, Direction, FetchOptions, Index, ObjectType,
    Oid, PushOptions, RemoteCallbacks, Repository, Signature, Tree,
};
use git2_credentials::CredentialHandler;
use log::{error, trace};
use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;
use thiserror::Error;
mod pull;

#[cfg(test)]
mod tests;
/// error for this crate
#[derive(Error, Debug)]
pub enum CodexGitError {
    #[error("git error")]
    Git {
        #[from]
        source: git2::Error,
        //  backtrace: Backtrace,
    },
    #[error("IO error")]
    IO(#[from] std::io::Error),
    #[error("RON error")]
    Ron(#[from] ron::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("codex git error")]
    CodexGit,
    #[error("utf8 error")]
    Utf8Error(std::str::Utf8Error),
}
/// results for this crate
pub type Result<T> = std::result::Result<T, CodexGitError>;
/// None or error
pub type NullResult = Result<()>;

/// tracing macro
macro_rules! git_trace {
    () => {  };
    ($($arg:tt)*) => {
        trace!("{} ({}:{})", Black.on(Cyan).paint(format!($($arg)*)), std::file!(), std::line!());
    };
}

/** An `SshKeys` stores the SSH keys for the remote repository. */
#[derive(Debug, Default, Clone, Setters, Deserialize)]
#[getset(set = "pub")]
pub struct SshKeys {
    /// public key
    public: String,
    /// private key
    private: String,
}
//impl SshKeys {}
/** `User` is a git user (user name and email)*/
#[derive(Debug, Default, Clone, Deserialize)]
pub struct User {
    name: String,
    email: String,
}
impl User {
    pub fn new(name: &str, email: &str) -> Self {
        Self {
            name: name.to_string(),
            email: email.to_string(),
        }
    }
}

/** A `CodexRepoConfig` is the parameters for making a [CodexRepository].  */
#[derive(Clone, Setters, Default, Deserialize, Debug)]
pub struct CodexRepoConfig {
    /// user name and email for Git commits
    #[getset(set = "pub")]
    user: User,
    /// URL for the remote repository
    #[getset(set = "pub")]
    remote_url: String,
    /// where to put the files on disk (excluding the repo name)
    #[getset(set = "pub")]
    path: PathBuf,
    /// paths to add automatically
    #[getset(set = "pub")]
    #[serde(default, skip)]
    auto_add: Vec<String>,
    /// SSH keys for the remote
    #[getset(set = "pub")]
    #[serde(default, skip_serializing)]
    ssh_keys: SshKeys,
    /// print more messages
    #[serde(default)]
    verbose: bool,
}
impl CodexRepoConfig {
    /** `repo_name` is the name of the repository */
    pub fn repo_name(&self) -> Result<String> {
        let parts = self.remote_url.split("/");
        Ok(parts.last().ok_or(CodexGitError::CodexGit)?.to_string())
    }
    /** `full_path` is the full path of the head of the repository on disk including the [Self::repo_name()] */
    pub fn full_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(format!(
            "{}/{}",
            self.path.to_string_lossy(),
            self.repo_name()?
        )))
    }
    /** `has_repository` detects whether a [CodexRepository] exists for this [CodexRepoConfig]. */
    pub fn has_repository(&self) -> Result<bool> {
        let repo_head = self.full_path()?;
        if !repo_head.exists() {
            git_trace!("repo does not exist {:?}", &repo_head);
            Ok(false)
        } else if !repo_head.is_dir() {
            error!("repo is not dir {:?}", &repo_head);
            Ok(false)
        } else {
            git_trace!("repo dir exists {:?}", &repo_head);
            Ok(true)
        }
    }
    /** `delete_repo` deletes the repository. */
    pub fn delete_repo(&self) -> Result<()> {
        std::fs::remove_dir_all(self.full_path()?)?;
        Ok(())
    }
    /** `clone_repo` creates a [CodexRepository] and clones the repository from the remote (a Git clone, not a Rust clone). */
    pub fn clone_repo(&mut self) -> Result<CodexRepository> {
        git_trace!("cloning repo {:?} to {:?}", &self.remote_url, &self.path);
        let fetch_options = self.fetch_options()?;
        let repo = RepoBuilder::new()
            .bare(false)
            .fetch_options(fetch_options)
            .clone(&self.remote_url, &self.full_path()?)?;
        git_trace!("repo cloned");
        Ok(CodexRepository::new(repo, self))
    }
    /** `open` opens an existing [CodexRepository]. */
    pub fn open(&self) -> Result<CodexRepository> {
        git_trace!("opening existing repo {:?}", &self.full_path()?);
        let repo = Repository::open(self.full_path()?)?;
        // git_trace!("repo opened");
        Ok(CodexRepository::new(repo, self))
    }
    /** `fetch_options` retrieves fetch options */
    fn fetch_options(&self) -> Result<FetchOptions> {
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(self.callbacks()?);
        Ok(fo)
    }
    /** `callbacks` sets callbacks for calls to git2 that use SSH */
    fn callbacks(&self) -> Result<RemoteCallbacks> {
        let mut cb = RemoteCallbacks::new();
        let git_config = git2::Config::open_default()?;
        let mut ch = CredentialHandler::new(git_config);
        let mut try_count: i8 = 0;
        const MAX_TRIES: i8 = 5;
        cb.credentials(move |url, username, allowed| {
            if allowed.contains(CredentialType::SSH_MEMORY) {
                git_trace!("trying ssh memory credential");
                let username = username.expect("no user name");
                //                git_trace!("user name is {}, using key option", &username);
                let cred_res = Cred::ssh_key_from_memory(
                    username,
                    Some(&self.ssh_keys.public),
                    &self.ssh_keys.private,
                    None,
                );
                git_trace!("try to find ssh memory credential");
                match &cred_res {
                    Err(e) => {
                        error!("error found in credential from memory {:?}", e);
                    }
                    Ok(_cr) => {
                        // git_trace!(
                        //     "found private: {}... public {}...",
                        //     self.ssh_keys.private.get(..10).unwrap_or_default(),
                        //     self.ssh_keys.public.get(..10).unwrap_or_default()
                        // );
                    }
                }
                return cred_res;
            }
            git_trace!("look for credential {:?} ({} tries)", allowed, try_count);
            try_count += 1;
            if try_count > MAX_TRIES {
                error!("too many tries for ssh key");
                std::panic::panic_any("too many ssh tries".to_string());
            }
            ch.try_next_credential(url, username, allowed)
        });

        // Print out our transfer progress.
        if self.verbose {
            cb.transfer_progress(|stats| {
                if stats.received_objects() == stats.total_objects() {
                    git_trace!(
                        "Resolving deltas {}/{} ",
                        stats.indexed_deltas(),
                        stats.total_deltas()
                    );
                } else if stats.total_objects() > 0 {
                    git_trace!(
                        "Received {}/{} objects ({}) in {} bytes ",
                        stats.received_objects(),
                        stats.total_objects(),
                        stats.indexed_objects(),
                        stats.received_bytes()
                    );
                }
                // io::stdout().flush().unwrap();
                true
            });
            cb.sideband_progress(|msg| {
                if msg.len() == 0 {
                    return true;
                }
                git_trace!(
                    "git: {}",
                    std::str::from_utf8(msg).unwrap_or_else(|err| {
                        error!("bad git utf8 message {:?}", &err);
                        "bad msg"
                    })
                );
                true
            });
        }
        Ok(cb)
    }
}
/** An `FetchStatus` describes the result of a fetch */
#[derive(Default, Getters, CopyGetters)]
pub struct FetchStatus {
    #[getset(get_copy = "pub")]
    is_changed: bool,
    #[getset(get = "pub")]
    index: Option<Index>,
}
impl FetchStatus {
    /** `has_conflict`  */
    pub fn has_conflict(&self) -> bool {
        if let Some(i) = &self.index {
            i.has_conflicts()
        } else {
            false
        }
    }
    // /** `has_changes` is whether there are any changes in the index */
    // pub fn has_changes(&self)->Result<bool> {unimplemented!()}
}
impl fmt::Display for FetchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "status (")?;
        if let Some(i) = &self.index {
            write!(f, "{} entries", i.len())?;
            if self.has_conflict() {
                write!(
                    f,
                    " {} conflicts",
                    i.conflicts().expect("bad conflicts").count()
                )?;
            }
        }
        write!(f, ")")
    }
}
impl fmt::Debug for FetchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ix_sz = if let Some(i) = &self.index {
            i.len()
        } else {
            0
        };
        let mut cc = Vec::<String>::new();
        let confict_msg = if self.has_conflict() {
            if let Some(ix) = &self.index {
                for conflict in ix.conflicts().expect("bad conflicts") {
                    if let Ok(c) = conflict {
                        let p = if let Some(our) = c.our {
                            std::str::from_utf8(&our.path).expect("bad utf").to_string()
                        } else {
                            "?".to_string()
                        };
                        cc.push(p);
                    }
                }
            }
            format!("conflicts [{}]", cc.join(" "))
        } else {
            "".to_string()
        };
        write!(
            f,
            "{} {} {} changes",
            &confict_msg,
            if self.is_changed {
                "changed"
            } else {
                "unchanged"
            },
            &ix_sz
        )?;
        if f.alternate() {
            if let Some(ix) = &self.index {
                write!(f, " [")?;
                for ie in ix.iter() {
                    write!(f, "{} ", std::str::from_utf8(&ie.path).expect("bad utf"))?;
                }
                write!(f, "]")?;
            }
        }
        Ok(())
    }
}
/** A `CodexRepository` is a Git [Repository] that is managed by this crate. It tracks whether the repository needs to be committed or pushed. */
pub struct CodexRepository {
    /// the underlying Git repository
    repo: Repository,
    /// configuration options
    config: CodexRepoConfig,
    /// repo has uncommitted changes
    needs_commit: bool,
    /// repo has unpushed commits
    needs_push: bool,
    /// added files
    added: Vec<String>,
}
impl fmt::Display for CodexRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} commit, {} push",
            if self.needs_commit {
                "needs"
            } else {
                "does not need"
            },
            if self.needs_push {
                "needs"
            } else {
                "does not need"
            }
        )
    }
}
impl Drop for CodexRepository {
    fn drop(&mut self) {
        git_trace!("at end (dropping repo), committing and pushing repo if required");
        self.commit_and_push().unwrap_or_else(|err| {
            error!("drop error: {:?}", &err);
            panic!("drop error")
        });
        // git_trace!("dropping.");
    }
}
impl fmt::Debug for CodexRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(CodexRepository)",)
    }
}
impl CodexRepository {
    /// initialise the repository based on the [CodexRepoConfig].
    pub fn new(repo: Repository, config: &CodexRepoConfig) -> Self {
        Self {
            repo,
            config: config.clone(),
            needs_commit: false,
            needs_push: false,
            added: vec![],
        }
    }
    /** `fetch` tries to update the local repository from the remote. */
    // pub fn fetch(&mut self) -> Result<FetchStatus> {
    //     git_trace!("fetching");
    //     let mut fetch_options = self.config.fetch_options()?;
    //     let mut remote = self.repo.find_remote("origin")?;
    //     git_trace!("actual fetch");
    //     remote.fetch(&["main"], Some(&mut fetch_options), None)?;
    //     git_trace!("fetched");
    //     let our_commit = self.our_commit()?;
    //     let their_commit = self.their_commit()?;
    //     if our_commit.id() == their_commit.id() {
    //         git_trace!(
    //             "same commit {} ({})",
    //             &our_commit.id().to_string()[..6],
    //             our_commit.summary().unwrap_or_default()
    //         );
    //         Ok(FetchStatus {
    //             is_changed: false,
    //             index: None,
    //         })
    //     } else {
    //         git_trace!(
    //             "our commit {} ({}) / their commit {} ({})",
    //             &our_commit.id().to_string()[..6],
    //             our_commit.summary().unwrap_or_default(),
    //             &their_commit.id().to_string()[..6],
    //             their_commit.summary().unwrap_or_default()
    //         );
    //         let index =
    //             self.repo
    //                 .merge_commits(&our_commit, &their_commit, Some(&MergeOptions::new()))?;
    //         Ok(FetchStatus {
    //             is_changed: true,
    //             index: Some(index),
    //         })
    //     }
    // }
    /// fetches data from the remote and merges if necessary
    pub fn fetch(&mut self) -> Result<()> {
        let remote_name = "origin";
        let remote_branch = "main";
        // let repo = Repository::open(".")?;
        let mut remote = self.repo.find_remote(remote_name)?;
        let fetch_commit = pull::do_fetch(
            &self.repo,
            &[remote_branch],
            &mut remote,
            self.config.callbacks()?,
        )?;
        pull::do_merge(&self.repo, &remote_branch, fetch_commit)?;
        Ok(())
    }
    // /** `commit_merge` commits any changes from a merge to the local repository. */
    // pub fn commit_merge(&mut self, fs: &mut FetchStatus) -> NullResult {
    //     if !fs.is_changed() {
    //         return Ok(());
    //     }
    //     git_trace!("committing merged");
    //     if let Some(i) = &mut fs.index {
    //         let new_tree = self.repo.find_tree(i.write_tree_to(&self.repo)?)?;
    //         let our_commit = self.our_commit()?;
    //         let their_commit = self.their_commit()?;
    //         let _oid =
    //             self.write_commit(new_tree, "merge commit", &[&our_commit, &their_commit])?;
    //         let obj = self.repo.revparse_single(&("refs/heads/main".to_owned()))?;
    //         self.repo.checkout_tree(&obj, None)?;
    //         self.repo.set_head(&("refs/heads/main".to_owned()))?;
    //     }
    //     self.needs_commit = false;
    //     self.needs_push = true;
    //     git_trace!("committed");
    //     Ok(())
    // }
    /** `commit_and_push` commits changes and pushes them */
    pub fn commit_and_push(&mut self) -> Result<()> {
        self.commit().context(format!(
            "error in commit ({} commit)",
            if self.needs_commit {
                "needs"
            } else {
                "does not need"
            }
        ))?;
        self.push(false).context(format!(
            "error in push ({} push)",
            if self.needs_push {
                "needs"
            } else {
                "does not need"
            }
        ))?;
        Ok(())
    }
    /** `commit` commits any changes to the local repository. */
    pub fn commit(&mut self) -> NullResult {
        if !self.needs_commit {
            git_trace!("no changes, do not need commit");
            return Ok(());
        }
        // TODO does this help?
        git_trace!("adding all from: {:?}", self.config.auto_add);
        let mut index = self.repo.index().context("cannot get the Index file")?;
        let mut paths = vec![];
        index.add_all(
            self.config.auto_add.iter(),
            IndexAddOption::DEFAULT,
            Some(&mut |path, spec| {
                paths.push(format!("{:?}", &path));
                git_trace!(
                    "adding for commit {:?} for {}",
                    &path,
                    std::str::from_utf8(spec).unwrap()
                );
                0
            }),
        )?;
        index.write().context("writing index for commit")?;
        //        git_trace!("committing");
        {
            let tree = self.repo.find_tree(self.repo.index()?.write_tree()?)?;
            let our_commit = self.our_commit()?;
            let _oid = self.write_commit(
                tree,
                &format!(
                    "commit changes {} {}",
                    paths.join(" "),
                    self.added.join(" ")
                ),
                &[&our_commit],
            )?;
        }
        self.added.clear();
        self.needs_commit = false;
        self.needs_push = true;
        //  git_trace!("committed");
        Ok(())
    }
    /** `write_commit` writes out a commit */
    fn write_commit(
        &self,
        new_tree: Tree<'_>,
        message: &str,
        parent_commits: &[&Commit<'_>],
    ) -> Result<Oid> {
        let update_ref = if parent_commits.len() > 0 {
            Some("HEAD")
        } else {
            None
        };
        let user = Signature::now(&self.config.user.name, &self.config.user.email)?;
        let commit_oid = self.repo.commit(
            update_ref,     //  point HEAD to our new commit
            &user,          // author
            &user,          // committer
            message,        // commit message
            &new_tree,      // tree
            parent_commits, // parents
        )?;
        Ok(commit_oid)
    }
    /** latest local commit for fetch */
    fn our_commit(&self) -> Result<Commit<'_>> {
        Ok(self.last_commit()?.ok_or_else(|| (anyhow!("no commit")))?)
    }
    /** `last_commit` finds the most recent commit or None */
    fn last_commit(&self) -> Result<Option<Commit>> {
        let head = self.repo.head()?.resolve()?.peel(ObjectType::Commit)?;
        Ok(Some(
            head.into_commit().map_err(|_e| anyhow!("not a commit"))?,
        ))
    }
    // /** latest commit on other branch after fetch */
    // fn their_commit(&self) -> Result<Commit<'_>> {
    //     let their_reference = self.repo.find_reference("FETCH_HEAD")?;
    //     Ok(their_reference.peel_to_commit()?)
    // }
    /** `add` adds a file to the index */
    pub fn add(&mut self, path: PathBuf) -> NullResult {
        git_trace!("adding {:?}", &path);
        self.repo.index()?.add_path(&path)?;
        self.needs_commit = true;
        self.added.push(path.to_string_lossy().to_string());
        Ok(())
    }
    /** `push` tries to push any local changes to the remote. */
    pub fn push(&mut self, force: bool) -> NullResult {
        if !self.needs_push {
            git_trace!("no commits, do not need push");
            return Ok(());
        }
        // let index = self.repo.index()?;
        // for i in 0..index.len() {
        //     git_trace!(
        //         "push index has {:?}",
        //         std::str::from_utf8(&index.get(i).unwrap().path)
        //             .map_err(|e| CodexGitError::Utf8Error(e))
        //     );
        // }
        git_trace!("pushing to remote");
        let mut remote = self.repo.find_remote("origin")?;
        let cb = self.config.callbacks()?;
        remote.connect_auth(Direction::Push, Some(cb), None)?;
        let mut push_options = PushOptions::new();
        let cb = self.config.callbacks()?;
        push_options.remote_callbacks(cb);
        let force_marker = if force { "+" } else { "" };
        let refspec = format!(
            "{}refs/heads/{}:refs/heads/{}",
            force_marker, "main", "main"
        );
        remote.push(&[refspec.as_str()], Some(&mut push_options))?;
        self.needs_push = false;
        git_trace!("pushed");
        Ok(())
    }
}
