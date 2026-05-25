use std::{
    env,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use git2::{
    build::{CheckoutBuilder, RepoBuilder},
    BranchType, IndexAddOption, Oid, Repository, RepositoryInitOptions, Signature,
};

pub fn current_dir_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub fn push(path: &Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

pub fn write_file(path: &Path, name: &str, contents: &str) {
    fs::write(PathBuf::from(path).join(name), contents).unwrap();
}

pub fn init_repo(path: &Path, initial_head: &str) -> Repository {
    let mut options = RepositoryInitOptions::new();
    options.initial_head(initial_head);
    Repository::init_opts(path, &options).unwrap()
}

pub fn clone_repo(source: &Path, destination: &Path) -> Repository {
    Repository::clone(source.to_str().unwrap(), destination).unwrap()
}

pub fn clone_bare_repo(source: &Path, destination: &Path) -> Repository {
    let mut builder = RepoBuilder::new();
    builder.bare(true);
    builder.clone(source.to_str().unwrap(), destination).unwrap()
}

pub fn configure_user(repo: &Repository) {
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Gitrex Test").unwrap();
    config.set_str("user.email", "gitrex@example.com").unwrap();
}

pub fn commit_all(repo: &Repository, message: &str) -> Oid {
    let sig = Signature::now("Gitrex Test", "gitrex@example.com").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();

    let parents = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .into_iter()
        .collect::<Vec<_>>();
    let parent_refs = parents.iter().collect::<Vec<_>>();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

pub fn checkout_branch(repo: &Repository, branch: &str) {
    let mut builder = CheckoutBuilder::new();
    builder.force();
    repo.set_head(&format!("refs/heads/{branch}")).unwrap();
    repo.checkout_head(Some(&mut builder)).unwrap();
}

pub fn create_branch(repo: &Repository, name: &str, target: &str) {
    let target_commit = repo.revparse_single(target).unwrap().peel_to_commit().unwrap();
    repo.branch(name, &target_commit, false).unwrap();
}

pub fn set_upstream(repo: &Repository, branch: &str, upstream: &str) {
    repo.find_branch(branch, BranchType::Local)
        .unwrap()
        .set_upstream(Some(upstream))
        .unwrap();
}

pub fn set_remote_head(repo: &Repository, reference: &str) {
    repo.reference_symbolic("HEAD", reference, true, "set remote head")
        .unwrap();
}

pub fn push_branch(repo: &Repository, remote_name: &str, branch: &str) {
    let mut remote = repo.find_remote(remote_name).unwrap();
    remote
        .push(
            &[format!("refs/heads/{branch}:refs/heads/{branch}")],
            None,
        )
        .unwrap();
}
