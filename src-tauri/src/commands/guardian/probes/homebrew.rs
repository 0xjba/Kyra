use std::path::{Path, PathBuf};

pub(crate) fn paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(home.join("Library/Caches/Homebrew"), "cache")]
}

pub(crate) fn outdated_count() -> u32 {
    let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|p| Path::new(p).exists());
    let Some(brew) = brew else {
        return 0;
    };
    match std::process::Command::new(brew)
        .args(["outdated", "--quiet"])
        .output()
    {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count() as u32,
        _ => 0,
    }
}
