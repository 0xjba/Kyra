use std::path::{Path, PathBuf};

pub(crate) fn cache_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            home.join("Library/Application Support/Slack/Cache"),
            "Slack cache",
        ),
        (
            home.join("Library/Application Support/discord/Cache"),
            "Discord cache",
        ),
        (
            home.join("Library/Application Support/discord/Code Cache"),
            "Discord code cache",
        ),
        (
            home.join("Library/Caches/com.spotify.client"),
            "Spotify cache",
        ),
        (
            home.join("Library/Caches/Google/Chrome/Default/Cache"),
            "Chrome cache",
        ),
        (
            home.join("Library/Caches/Firefox/Profiles"),
            "Firefox cache",
        ),
        (home.join("Library/Caches/com.apple.Safari"), "Safari cache"),
        (
            home.join("Library/Application Support/Microsoft/Teams/Cache"),
            "Teams cache",
        ),
    ]
}

pub(crate) fn data_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (
            home.join("Library/Application Support/Spotify/PersistentCache"),
            "Spotify offline",
        ),
        (
            home.join("Library/Application Support/zoom.us/data"),
            "Zoom data",
        ),
    ]
}
