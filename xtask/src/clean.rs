use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;

pub struct CleanConfig {
    pub workspaces: PathBuf,
    pub kinds: Vec<String>,
    pub conversations: Option<BTreeSet<String>>,
}

pub fn resolve_kinds(kind: &str) -> Vec<String> {
    if kind == "all" {
        return vec!["curated".to_string(), "fs".to_string(), "iwe".to_string()];
    }
    kind.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn run(config: &CleanConfig) -> Result<()> {
    let mut removed = 0;
    for kind in &config.kinds {
        let root = config.workspaces.join(kind);
        if !root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let base = name.strip_suffix(".snap").unwrap_or(&name);
            if let Some(filter) = &config.conversations {
                if !filter.contains(base) {
                    continue;
                }
            }
            fs::remove_dir_all(&path)?;
            println!("removed {}", path.display());
            removed += 1;
        }
    }
    println!("removed {} workspace(s)", removed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_all_resolves() {
        assert_eq!(resolve_kinds("all"), vec!["curated", "fs", "iwe"]);
    }

    #[test]
    fn kind_list_resolves() {
        assert_eq!(resolve_kinds("curated,fs"), vec!["curated", "fs"]);
    }

    #[test]
    fn removes_matching_workspaces_and_snapshots() {
        let root = std::env::temp_dir().join(format!("clean-test-{}", std::process::id()));
        let curated = root.join("curated");
        fs::create_dir_all(curated.join("conv-1")).unwrap();
        fs::create_dir_all(curated.join("conv-1.snap")).unwrap();
        fs::create_dir_all(curated.join("conv-2")).unwrap();
        run(&CleanConfig {
            workspaces: root.clone(),
            kinds: vec!["curated".to_string()],
            conversations: Some(BTreeSet::from(["conv-1".to_string()])),
        })
        .unwrap();
        assert!(!curated.join("conv-1").exists());
        assert!(!curated.join("conv-1.snap").exists());
        assert!(curated.join("conv-2").exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
