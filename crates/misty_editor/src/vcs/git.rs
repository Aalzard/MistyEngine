//! The `git` command line, run as a child process (no libgit2): repository discovery, status,
//! diffs, stage / unstage, commit and discard. Every call names the project folder with `-C`, so
//! the editor's own working directory never matters.
//!
//! Tests only ever run git inside the system temp folder (see [`git`]).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::status::{self, Status};

/// A git work tree holding the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// The work tree's top folder (`git rev-parse --show-toplevel`).
    pub root: PathBuf,
    /// The project folder.
    pub project: PathBuf,
    /// The project folder relative to [`Repo::root`], with a trailing `/` ("" when the project
    /// is the repository's top folder).
    pub prefix: String,
}

/// Run `git -C dir <args>` and return its standard output (trimmed of nothing). A non-zero exit
/// gives standard error (or output) as the message.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = command(dir, args)?.output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            "git was not found (install Git and restart the editor)".to_owned()
        }
        _ => format!("git: {e}"),
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if err.is_empty() {
            stdout.trim().to_owned()
        } else {
            err
        })
    }
}

/// The git command for `dir`: no pager, no colours, paths as they are, no console window.
fn command(dir: &Path, args: &[&str]) -> Result<Command, String> {
    guard(dir)?;
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(["-c", "core.quotepath=off", "-c", "color.ui=never"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    if cfg!(test) {
        // A temp repo's search for `.git` stops at the temp folder: never a repo above it.
        c.env("GIT_CEILING_DIRECTORIES", std::env::temp_dir());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(c)
}

/// Tests run git only inside the temp folder, never against a real repository.
fn guard(dir: &Path) -> Result<(), String> {
    if !cfg!(test) {
        return Ok(());
    }
    let temp = std::env::temp_dir();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canon(dir).starts_with(canon(&temp)) {
        Ok(())
    } else {
        Err(format!(
            "tests run git only under the temp folder (not {})",
            dir.display()
        ))
    }
}

/// The repository holding `project`, or why there is none.
pub fn discover(project: &Path) -> Result<Repo, String> {
    let out = git(project, &["rev-parse", "--show-toplevel", "--show-prefix"])?;
    let mut lines = out.lines();
    let root = lines
        .next()
        .filter(|l| !l.is_empty())
        .ok_or("not a git repository")?;
    let prefix = lines.next().unwrap_or("").trim().to_owned();
    Ok(Repo {
        root: PathBuf::from(root),
        project: project.to_path_buf(),
        prefix,
    })
}

/// Text assets get a diff; anything else is listed as binary.
pub fn is_text_asset(rel: &str) -> bool {
    const TEXT: &[&str] = &[
        "lua",
        "scene",
        "prefab",
        "mat",
        "json",
        "txt",
        "md",
        "wgsl",
        "shader",
        "anim",
        "controller",
        "meta",
        "mixer",
        "timeline",
        "inputactions",
        "shadergraph",
        "effect",
        "csv",
        "xml",
        "yaml",
        "yml",
        "toml",
        "cfg",
        "ini",
        "physicmaterial",
        "terrainlayer",
        "overridecontroller",
        "mask",
        "signal",
        "preset",
        "spriteatlas",
        "fontasset",
        "localization",
        "gitignore",
        "gitattributes",
        "html",
        "css",
        "js",
    ];
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let ext = name.rsplit_once('.').map_or(name, |(_, e)| e);
    TEXT.iter().any(|t| ext.eq_ignore_ascii_case(t))
}

impl Repo {
    /// The project-relative path as git names it (relative to the repository's top).
    pub fn repo_path(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }

    /// Branch and changed files under the project folder.
    pub fn status(&self) -> Result<Status, String> {
        let out = git(
            &self.project,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--branch",
                "--untracked-files=all",
                "--",
                ".",
            ],
        )?;
        Ok(status::parse(&out, &self.prefix))
    }

    /// Unified diff of one file: staged (index against HEAD) or not (work tree against index).
    /// Untracked files show their whole text as added.
    pub fn diff(&self, rel: &str, staged: bool, untracked: bool) -> Result<String, String> {
        if untracked {
            let text =
                std::fs::read_to_string(self.project.join(rel)).map_err(|e| e.to_string())?;
            let mut out = format!("new file {rel}\n@@ -0,0 +1,{} @@\n", text.lines().count());
            for line in text.lines() {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
            return Ok(out);
        }
        let path = self.repo_path(rel);
        let mut args = vec!["diff", "--no-ext-diff", "--no-color"];
        if staged {
            args.push("--cached");
        }
        args.extend(["--", path.as_str()]);
        git(&self.root, &args)
    }

    /// A file's text at a revision: `HEAD`, or `:1` / `:2` / `:3` for a conflict's base, ours and
    /// theirs.
    pub fn show(&self, rev: &str, rel: &str) -> Result<String, String> {
        let spec = format!("{rev}:{}", self.repo_path(rel));
        git(&self.root, &["show", &spec])
    }

    pub fn stage(&self, rel: &str) -> Result<(), String> {
        git(&self.root, &["add", "--", &self.repo_path(rel)]).map(drop)
    }

    pub fn unstage(&self, rel: &str) -> Result<(), String> {
        let path = self.repo_path(rel);
        match git(&self.root, &["restore", "--staged", "--", &path]) {
            Ok(_) => Ok(()),
            // No commit yet: there is no HEAD to restore the index from.
            Err(_) => git(&self.root, &["rm", "--cached", "--quiet", "--", &path]).map(drop),
        }
    }

    /// Throw away a file's changes: back to HEAD, or removed when git didn't have it before.
    pub fn discard(&self, rel: &str, index: char, worktree: char) -> Result<(), String> {
        let path = self.repo_path(rel);
        let file = self.project.join(rel);
        match (index, worktree) {
            ('?', '?') => remove_file(&file),
            ('A', _) => {
                git(
                    &self.root,
                    &["rm", "--cached", "--quiet", "--force", "--", &path],
                )?;
                remove_file(&file)
            }
            _ => git(
                &self.root,
                &[
                    "restore",
                    "--source=HEAD",
                    "--staged",
                    "--worktree",
                    "--",
                    &path,
                ],
            )
            .map(drop),
        }
    }

    /// Resolve a conflicted file with one whole side and stage it.
    pub fn take_side(&self, rel: &str, ours: bool) -> Result<(), String> {
        let path = self.repo_path(rel);
        let side = if ours { "--ours" } else { "--theirs" };
        git(&self.root, &["checkout", side, "--", &path])?;
        self.stage(rel)
    }

    /// Write a merged file and stage it (marks the conflict resolved).
    pub fn resolve_with(&self, rel: &str, text: &str) -> Result<(), String> {
        std::fs::write(self.project.join(rel), text).map_err(|e| e.to_string())?;
        self.stage(rel)
    }

    /// Commit what is staged; returns git's one-line summary.
    pub fn commit(&self, message: &str) -> Result<String, String> {
        let out = git(&self.root, &["commit", "--quiet", "-m", message])?;
        let summary = git(&self.root, &["log", "-1", "--format=%h %s"]).unwrap_or(out);
        Ok(summary.trim().to_owned())
    }
}

/// An untracked file goes to the system trash (tests just delete it).
fn remove_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if cfg!(test) {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    } else {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh repository in the temp folder with a committed `Proj/Assets` project inside;
    /// returns (repo top, project folder).
    pub fn temp_repo(tag: &str) -> (PathBuf, PathBuf) {
        let top = std::env::temp_dir().join(format!("misty_vcs_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let project = top.join("Proj");
        std::fs::create_dir_all(project.join("Assets/Scripts")).unwrap();
        git(&top, &["init", "--quiet", "-b", "main"]).unwrap();
        for (k, v) in [
            ("user.name", "Misty Test"),
            ("user.email", "test@misty.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            git(&top, &["config", k, v]).unwrap();
        }
        std::fs::write(
            project.join("Assets/Scripts/Player.lua"),
            "local speed = 1\n",
        )
        .unwrap();
        std::fs::write(project.join("Assets/Readme.txt"), "hello\n").unwrap();
        git(&top, &["add", "--", "Proj"]).unwrap();
        git(&top, &["commit", "--quiet", "-m", "first"]).unwrap();
        (top, project)
    }

    #[test]
    fn discovers_the_repo_and_the_project_prefix() {
        let (top, project) = temp_repo("discover");
        let repo = discover(&project).unwrap();
        assert_eq!(repo.prefix, "Proj/");
        assert_eq!(repo.repo_path("Assets/a.lua"), "Proj/Assets/a.lua");
        assert!(std::fs::canonicalize(&repo.root).unwrap() == std::fs::canonicalize(&top).unwrap());
        // A folder with no repository (the ceiling stops the search at the temp folder).
        let lone = std::env::temp_dir().join(format!("misty_vcs_lone_{}", std::process::id()));
        std::fs::create_dir_all(&lone).unwrap();
        assert!(discover(&lone).is_err());
        std::fs::remove_dir_all(lone).ok();
        std::fs::remove_dir_all(top).ok();
    }

    #[test]
    fn git_refuses_folders_outside_temp_in_tests() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let err = git(&here, &["status"]).unwrap_err();
        assert!(err.contains("temp folder"), "{err}");
    }

    #[test]
    fn stage_unstage_commit_and_discard() {
        let (top, project) = temp_repo("flow");
        let repo = discover(&project).unwrap();
        std::fs::write(
            project.join("Assets/Scripts/Player.lua"),
            "local speed = 2\n",
        )
        .unwrap();
        std::fs::write(project.join("Assets/New.lua"), "print(1)\n").unwrap();
        let s = repo.status().unwrap();
        assert_eq!(s.branch, "main");
        let player = s.entry("Assets/Scripts/Player.lua").unwrap();
        assert_eq!((player.index, player.worktree), (' ', 'M'));
        assert!(s.entry("Assets/New.lua").unwrap().is_untracked());

        let diff = repo
            .diff("Assets/Scripts/Player.lua", false, false)
            .unwrap();
        assert!(
            diff.contains("-local speed = 1") && diff.contains("+local speed = 2"),
            "{diff}"
        );
        let new = repo.diff("Assets/New.lua", false, true).unwrap();
        assert!(new.contains("+print(1)"));

        repo.stage("Assets/Scripts/Player.lua").unwrap();
        let staged = repo.diff("Assets/Scripts/Player.lua", true, false).unwrap();
        assert!(staged.contains("+local speed = 2"));
        assert!(
            repo.status()
                .unwrap()
                .entry("Assets/Scripts/Player.lua")
                .unwrap()
                .is_staged()
        );
        repo.unstage("Assets/Scripts/Player.lua").unwrap();
        assert!(
            !repo
                .status()
                .unwrap()
                .entry("Assets/Scripts/Player.lua")
                .unwrap()
                .is_staged()
        );

        repo.stage("Assets/New.lua").unwrap();
        let summary = repo.commit("Add New.lua").unwrap();
        assert!(summary.ends_with("Add New.lua"), "{summary}");
        let s = repo.status().unwrap();
        assert!(s.entry("Assets/New.lua").is_none(), "committed");
        assert_eq!(repo.show("HEAD", "Assets/New.lua").unwrap(), "print(1)\n");

        // Discard: a modified file goes back to HEAD, an untracked one is removed.
        repo.discard("Assets/Scripts/Player.lua", ' ', 'M').unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join("Assets/Scripts/Player.lua")).unwrap(),
            "local speed = 1\n"
        );
        std::fs::write(project.join("Assets/Junk.txt"), "x").unwrap();
        repo.discard("Assets/Junk.txt", '?', '?').unwrap();
        assert!(!project.join("Assets/Junk.txt").exists());
        // A staged new file: out of the index and gone.
        std::fs::write(project.join("Assets/Staged.txt"), "x").unwrap();
        repo.stage("Assets/Staged.txt").unwrap();
        repo.discard("Assets/Staged.txt", 'A', ' ').unwrap();
        assert!(!project.join("Assets/Staged.txt").exists());
        assert!(repo.status().unwrap().entries.is_empty());
        std::fs::remove_dir_all(top).ok();
    }

    #[test]
    fn text_assets() {
        assert!(is_text_asset("Assets/Scenes/A.scene"));
        assert!(is_text_asset("Assets/x.LUA"));
        assert!(!is_text_asset("Assets/Textures/a.png"));
        assert!(!is_text_asset("Assets/Audio/a.ogg"));
    }
}
