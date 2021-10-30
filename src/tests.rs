/*! tests for parent.

Only run one test at a time.

initialise test remote repo using make-test-repos.sh script*/
use super::*;
use anyhow::Result;
use chrono::Local;
use ron::ser::{to_writer_pretty, PrettyConfig};
use std::env::{current_dir, temp_dir};
use std::fs::create_dir_all;
use std::fs::File;
use tempfile::tempdir;
/// tracing macro
macro_rules! git_test_trace {
    () => {  };
    ($($arg:tt)*) => {
        trace!("{} ({}:{})", Cyan.on(Black).paint(format!($($arg)*)), std::file!(), std::line!());
    };
}

#[test]
fn basic() -> NullResult {
    let _ = simple_logger::init();
    git_test_trace!("b: starting basic test");
    let mut config = test_config()?;
    assert!(!config.has_repository()?, "at {:?}", &config.full_path());
    let test_file_name = format!("{}/{}", config.full_path()?.to_string_lossy(), "test.txt");
    let test_data = format!("now is {:?} from prog", Local::now());
    git_test_trace!("b: test data is '{}'", &test_data);
    {
        git_test_trace!("--- creating repo, writing to test.txt ---");
        git_test_trace!("b: cloning repo");
        let mut codex_repo = config.clone_repo()?;
        assert!(config.has_repository()?, "at {:?}", &config.path);
        {
            let out_file = File::create(&test_file_name)?;
            to_writer_pretty(out_file, &test_data, PrettyConfig::new())?;
        }
        codex_repo.add(PathBuf::from("test.txt"))?;
        git_test_trace!("b: commiting modified data");
        codex_repo.commit()?;
        git_test_trace!("b: pushing");
        codex_repo.push(false)?;
    }

    {
        git_test_trace!("--- opening repo, reading test.txt ---");
        git_test_trace!("b: reopening repo");
        let mut codex_repo = config.open().context("basic#a")?;
        let status = codex_repo.fetch().context("basic#b")?;
        git_test_trace!("b: merge status is {:?}", &status);
        check_file(&test_file_name, &test_data).context("basic#c")?;
    }
    {
        git_test_trace!("--- cloning repo, reading test.txt ---");
        git_test_trace!("b: recloning repo to a different local repository");
        config.path = tempdir().context("basic#d")?.path().to_path_buf();
        let _codex_repo = config.clone_repo().context("basic#e")?;
        check_file(&test_file_name, &test_data).context("basic#f")?;
    }
    git_test_trace!("b: basic test complete");
    Ok(())
}
#[test]
/// test with several repositories
fn multirepo() -> NullResult {
    let _ = simple_logger::init();
    git_test_trace!("mr: starting multirepo test");
    let mut config = test_config()?;
    assert!(!config.has_repository()?, "at {:?}", &config.full_path());
    let file_name = "test2.txt";
    let test_file_name1 = format!("{}/{}", config.full_path()?.to_string_lossy(), &file_name);
    git_test_trace!("mr: test file name is {}", &test_file_name1);
    let test_data_old = format!("mr/old is {:?} from prog", Local::now());
    git_test_trace!("mr: old test data is '{}'", &test_data_old);
    let test_data_new = format!("mr/new is {:?} from prog", Local::now());
    git_test_trace!("mr: new test data is '{}'", &test_data_new);

    {
        git_test_trace!(
            "--- cloning first repo, writing old data to {} ---",
            &file_name
        );
        git_test_trace!("mr: cloning repo 1");
        let mut codex_repo = config.clone_repo()?;
        assert!(config.has_repository()?, "at {:?}", &config.path);
        {
            let out_file = File::create(&test_file_name1)?;
            to_writer_pretty(out_file, &test_data_old, PrettyConfig::new())?;
        }
        check_file(&test_file_name1, &test_data_old)?;
        codex_repo.add(PathBuf::from(file_name))?;
    }

    {
        git_test_trace!(
            "--- cloning second repo, writing new data to {} ---",
            &file_name
        );
        git_test_trace!("mr: cloning repo 2");
        let mut config2 = config.clone();
        config2.path = tempdir()?.path().to_path_buf();
        let test_file_name2 = format!("{}/{}", config2.full_path()?.to_string_lossy(), &file_name);
        git_test_trace!("mr: test file name is {}", &test_file_name2);
        let mut codex_repo2 = config2.clone_repo()?;
        assert!(config2.has_repository()?, "at {:?}", &config2.path);
        check_file(&test_file_name2, &test_data_old)?;
        {
            let out_file = File::create(&test_file_name2)?;
            to_writer_pretty(out_file, &test_data_new, PrettyConfig::new())?;
        }
        check_file(&test_file_name2, &test_data_new)?;
        codex_repo2.add(PathBuf::from(file_name))?;
    }

    {
        git_test_trace!(
            "--- opening first repo, checking data in {} ---",
            &file_name
        );
        git_test_trace!("mr: reopening repo");
        let mut codex_repo = config.open()?;
        let status = codex_repo.fetch()?;
        git_test_trace!("mr: merge status is {:?}", &status);
        check_file(&test_file_name1, &test_data_new)?;
    }
    git_test_trace!("multirepo test complete");
    Ok(())
}
fn check_file(file_name: &str, expected_contents: &str) -> anyhow::Result<()> {
    let in_file = File::open(&file_name)?;
    let in_data: String = ron::de::from_reader(in_file)?;
    assert_eq!(expected_contents, in_data, "file name: {}", &file_name);
    Ok(())
}
fn test_config() -> Result<CodexRepoConfig> {
    let test_dir = tempdir()?;
    let path = test_dir.path();
    git_test_trace!("test dir path is {}", &path.to_string_lossy());
    create_dir_all(&path)?;
    assert!(path.is_dir());
    let current_directory = current_dir()?;
    let temp_dir_str: String = temp_dir().to_string_lossy().to_string();
    let remote_url = format!("file://{}/codex-test/remote", &temp_dir_str);
    git_test_trace!(
        "current dir is {:?}, remote URL is {}",
        &current_directory,
        &remote_url
    );
    let config = CodexRepoConfig {
        user: User::new("tester", "tester@example.com"),
        remote_url: remote_url,
        path: path.to_path_buf(),
        ssh_keys: SshKeys {
            private: "".to_string(),
            public: "".to_string(),
        },
        auto_add: vec![".".to_string()],
        verbose: false,
    };
    Ok(config)
}
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

