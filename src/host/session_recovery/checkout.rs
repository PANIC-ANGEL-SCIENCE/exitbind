//! Product checkout trees with uncommitted changes, named after a resume.
//!
//! A host resume can end a worker that left edits behind. This reads the
//! configured product root's Git worktrees (the main checkout and every
//! `git worktree list` entry) and reports which hold uncommitted changes. It
//! never stages, cleans or locks anything (`GIT_OPTIONAL_LOCKS=0`), and a
//! missing or failing Git is skipped quietly.

use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

pub(super) const HOOK_TREE_LIMIT: usize = 4;
pub(super) const SHOW_TREE_LIMIT: usize = 16;
/// Worktrees inspected per repository, dirty or clean.
const MAX_WORKTREES: usize = 32;
const MAX_LIST_BYTES: usize = 64 * 1024;
const MAX_STATUS_BYTES: usize = 1024 * 1024;

pub(super) struct Tree {
    pub(super) path: String,
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) changed: usize,
}

pub(super) struct Trees {
    pub(super) dirty: Vec<Tree>,
    /// Dirty trees beyond the requested limit.
    pub(super) more: usize,
    pub(super) complete: bool,
}

/// Dirty worktrees of the repositories holding `roots`, each listed once.
pub(super) fn uncommitted(roots: &[PathBuf], limit: usize, deadline: Instant) -> Trees {
    let mut trees = Trees {
        dirty: Vec::new(),
        more: 0,
        complete: true,
    };
    let mut seen = BTreeSet::new();
    for root in roots {
        let Some(listed) = worktrees(root, deadline) else {
            trees.complete &= Instant::now() < deadline;
            continue;
        };
        for mut tree in listed {
            if !seen.insert(tree.path.clone()) {
                continue;
            }
            let Some(changed) = changed_paths(Path::new(&tree.path), deadline) else {
                trees.complete &= Instant::now() < deadline;
                continue;
            };
            if changed == 0 {
                continue;
            }
            if trees.dirty.len() >= limit {
                trees.more += 1;
                continue;
            }
            tree.changed = changed;
            trees.dirty.push(tree);
        }
    }
    trees
}

pub(super) fn show_value(trees: &Trees) -> Value {
    json!({
        "trees":trees.dirty.iter().map(|tree| json!({
            "path":tree.path,"branch":tree.branch,"head":tree.head,"changedPaths":tree.changed
        })).collect::<Vec<_>>(),
        "moreDirtyTrees":trees.more,
        "complete":trees.complete,
    })
}

fn git(directory: &Path, args: &[&str], cap: usize, deadline: Instant) -> Option<Vec<u8>> {
    let mut command = Command::new("git");
    crate::host::codegraph::clear_git_environment(&mut command);
    command.arg("-C").arg(directory).args(args);
    let output = crate::kernel::bounded_process::run(&mut command, cap, 4096, deadline).ok()?;
    output.status.success().then_some(output.stdout)
}

fn worktrees(root: &Path, deadline: Instant) -> Option<Vec<Tree>> {
    let listed = git(
        root,
        &["worktree", "list", "--porcelain", "-z"],
        MAX_LIST_BYTES,
        deadline,
    )?;
    Some(parse_worktrees(&listed))
}

/// `git worktree list --porcelain -z`: NUL-terminated attribute lines, with
/// an empty line between worktrees.
fn parse_worktrees(listed: &[u8]) -> Vec<Tree> {
    let mut out = Vec::new();
    let mut current: Option<Tree> = None;
    for line in listed.split(|byte| *byte == 0) {
        let Ok(line) = std::str::from_utf8(line) else {
            current = None;
            continue;
        };
        if line.is_empty() {
            out.extend(current.take());
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            out.extend(current.take());
            current = Some(Tree {
                path: path.to_owned(),
                branch: None,
                head: None,
                changed: 0,
            });
        } else if let Some(tree) = current.as_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                tree.head = Some(head.chars().take(12).collect());
            } else if let Some(branch) = line.strip_prefix("branch ") {
                tree.branch = Some(branch.trim_start_matches("refs/heads/").to_owned());
            } else if line == "bare" || line.starts_with("prunable") {
                current = None;
            }
        }
    }
    out.extend(current);
    out.retain(|tree| Path::new(&tree.path).is_dir());
    out.truncate(MAX_WORKTREES);
    out
}

/// Changed paths, staged or not, including untracked files a worker added.
fn changed_paths(tree: &Path, deadline: Instant) -> Option<usize> {
    let status = git(
        tree,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
        MAX_STATUS_BYTES,
        deadline,
    )?;
    let mut count = 0;
    let mut records = status
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(record) = records.next() {
        count += 1;
        // A rename or copy carries its source path as the next record.
        if record.len() > 2 && (record[..2].contains(&b'R') || record[..2].contains(&b'C')) {
            records.next();
        }
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_listing_keeps_branch_and_detached_trees_and_skips_bare_or_prunable() {
        let root = crate::test_root::temp("worktree-listing");
        let root = root.to_str().unwrap();
        let listed = format!(
            "worktree {root}\0HEAD 0123456789abcdef0123\0branch refs/heads/feature/x\0\0\
             worktree {root}\0HEAD fedcba9876543210fedc\0detached\0\0\
             worktree /nonexistent-exitbind-tree\0HEAD 00\0branch refs/heads/gone\0\0\
             worktree {root}\0bare\0\0\
             worktree {root}\0HEAD aa\0prunable gitdir file points to non-existent location\0\0"
        );
        let trees = parse_worktrees(listed.as_bytes());
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[0].branch.as_deref(), Some("feature/x"));
        assert_eq!(trees[0].head.as_deref(), Some("0123456789ab"));
        assert_eq!(trees[1].branch, None);
        assert_eq!(trees[1].head.as_deref(), Some("fedcba987654"));
    }
}
