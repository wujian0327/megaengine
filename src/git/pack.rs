use anyhow::Result;
use git2::Repository;
use std::process::Command;

/// Pack a git repository into a single file using git bundle
/// This creates a bundle file that contains all branches and commits
///
/// # Arguments
/// * `repo_path` - Path to the git repository to pack
/// * `output_path` - Path where the bundle file will be created
///
/// # Example
/// ```ignore
/// pack_repo_bundle("/path/to/repo", "/tmp/repo.bundle")?;
/// ```
pub fn pack_repo_bundle(repo_path: &str, output_path: &str) -> Result<()> {
    let repo = Repository::open(repo_path)
        .map_err(|e| anyhow::anyhow!("failed to open git repo: {}", e))?;

    // Get all branches to include in the bundle
    let mut branch_refs = Vec::new();
    let mut branches = repo
        .branches(None)
        .map_err(|e| anyhow::anyhow!("failed to list branches: {}", e))?;

    while let Some(branch_result) = branches.next() {
        let (branch, _) =
            branch_result.map_err(|e| anyhow::anyhow!("failed to get branch: {}", e))?;
        if let Ok(name) = branch.name() {
            if let Some(name_str) = name {
                branch_refs.push(name_str.to_string());
            }
        }
    }

    // If no branches found, try to get HEAD
    if branch_refs.is_empty() {
        if repo.head().is_ok() {
            branch_refs.push("HEAD".to_string());
        }
    }

    if branch_refs.is_empty() {
        return Err(anyhow::anyhow!("no branches found to bundle"));
    }

    // Use git bundle command to create the bundle
    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path)
        .arg("bundle")
        .arg("create")
        .arg(output_path);

    for branch_ref in &branch_refs {
        cmd.arg(branch_ref);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("failed to execute git bundle: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git bundle failed: {}", stderr));
    }

    Ok(())
}
