use super::CleanRule;

pub fn all_rules() -> Vec<CleanRule> {
    vec![
        // ── System ──────────────────────────────────────────
        CleanRule {
            id: "system_caches".into(),
            category: "System".into(),
            label: "System Caches".into(),
            paths: vec!["/Library/Caches".into()],
        },
        CleanRule {
            id: "system_tmp".into(),
            category: "System".into(),
            label: "Temporary Files".into(),
            paths: vec!["/private/tmp".into(), "/private/var/tmp".into()],
        },
        CleanRule {
            id: "system_logs".into(),
            category: "System".into(),
            label: "System Logs".into(),
            paths: vec!["/private/var/log".into()],
        },
        CleanRule {
            id: "crash_reports".into(),
            category: "System".into(),
            label: "Crash Reports".into(),
            paths: vec![
                "~/Library/Logs/DiagnosticReports".into(),
                "/Library/Logs/DiagnosticReports".into(),
            ],
        },
        // ── User ────────────────────────────────────────────
        CleanRule {
            id: "user_caches".into(),
            category: "User".into(),
            label: "User Caches".into(),
            paths: vec!["~/Library/Caches".into()],
        },
        CleanRule {
            id: "user_logs".into(),
            category: "User".into(),
            label: "User Logs".into(),
            paths: vec!["~/Library/Logs".into()],
        },
        CleanRule {
            id: "trash".into(),
            category: "User".into(),
            label: "Trash".into(),
            paths: vec!["~/.Trash".into()],
        },
        // ── Browsers ────────────────────────────────────────
        CleanRule {
            id: "safari_cache".into(),
            category: "Browsers".into(),
            label: "Safari Cache".into(),
            paths: vec![
                "~/Library/Caches/com.apple.Safari".into(),
                "~/Library/Caches/com.apple.Safari.SearchHelper".into(),
            ],
        },
        CleanRule {
            id: "chrome_cache".into(),
            category: "Browsers".into(),
            label: "Chrome Cache".into(),
            paths: vec![
                "~/Library/Caches/Google/Chrome".into(),
                "~/Library/Application Support/Google/Chrome/Default/Cache".into(),
                "~/Library/Application Support/Google/Chrome/Default/Code Cache".into(),
                "~/Library/Application Support/Google/Chrome/Default/GPUCache".into(),
            ],
        },
        CleanRule {
            id: "firefox_cache".into(),
            category: "Browsers".into(),
            label: "Firefox Cache".into(),
            paths: vec!["~/Library/Caches/Firefox".into()],
        },
        CleanRule {
            id: "edge_cache".into(),
            category: "Browsers".into(),
            label: "Edge Cache".into(),
            paths: vec!["~/Library/Caches/Microsoft Edge".into()],
        },
        CleanRule {
            id: "brave_cache".into(),
            category: "Browsers".into(),
            label: "Brave Cache".into(),
            paths: vec!["~/Library/Caches/BraveSoftware".into()],
        },
        CleanRule {
            id: "arc_cache".into(),
            category: "Browsers".into(),
            label: "Arc Cache".into(),
            paths: vec!["~/Library/Caches/company.thebrowser.Browser".into()],
        },
        // ── Developer Tools ─────────────────────────────────
        CleanRule {
            id: "npm_cache".into(),
            category: "Developer Tools".into(),
            label: "npm Cache".into(),
            paths: vec!["~/.npm/_cacache".into()],
        },
        CleanRule {
            id: "yarn_cache".into(),
            category: "Developer Tools".into(),
            label: "Yarn Cache".into(),
            paths: vec!["~/Library/Caches/Yarn".into(), "~/.cache/yarn".into()],
        },
        CleanRule {
            id: "pnpm_cache".into(),
            category: "Developer Tools".into(),
            label: "pnpm Cache".into(),
            paths: vec!["~/Library/pnpm/store".into(), "~/.local/share/pnpm/store".into()],
        },
        CleanRule {
            id: "bun_cache".into(),
            category: "Developer Tools".into(),
            label: "Bun Cache".into(),
            paths: vec!["~/.bun/install/cache".into()],
        },
        CleanRule {
            id: "cargo_cache".into(),
            category: "Developer Tools".into(),
            label: "Cargo Cache (Rust)".into(),
            paths: vec!["~/.cargo/registry/cache".into()],
        },
        CleanRule {
            id: "pip_cache".into(),
            category: "Developer Tools".into(),
            label: "pip Cache (Python)".into(),
            paths: vec!["~/Library/Caches/pip".into(), "~/.cache/pip".into()],
        },
        CleanRule {
            id: "go_cache".into(),
            category: "Developer Tools".into(),
            label: "Go Build Cache".into(),
            paths: vec!["~/Library/Caches/go-build".into()],
        },
        CleanRule {
            id: "gradle_cache".into(),
            category: "Developer Tools".into(),
            label: "Gradle Cache".into(),
            paths: vec!["~/.gradle/caches".into()],
        },
        CleanRule {
            id: "maven_cache".into(),
            category: "Developer Tools".into(),
            label: "Maven Cache".into(),
            paths: vec!["~/.m2/repository".into()],
        },
        CleanRule {
            id: "cocoapods_cache".into(),
            category: "Developer Tools".into(),
            label: "CocoaPods Cache".into(),
            paths: vec!["~/Library/Caches/CocoaPods".into()],
        },
        CleanRule {
            id: "xcode_derived".into(),
            category: "Developer Tools".into(),
            label: "Xcode DerivedData".into(),
            paths: vec!["~/Library/Developer/Xcode/DerivedData".into()],
        },
        CleanRule {
            id: "xcode_archives".into(),
            category: "Developer Tools".into(),
            label: "Xcode Archives".into(),
            paths: vec!["~/Library/Developer/Xcode/Archives".into()],
        },
        CleanRule {
            id: "docker_cache".into(),
            category: "Developer Tools".into(),
            label: "Docker Cache".into(),
            paths: vec![
                "~/Library/Containers/com.docker.docker/Data/vms".into(),
                "~/.docker/buildx".into(),
            ],
        },
        // ── Applications ────────────────────────────────────
        CleanRule {
            id: "discord_cache".into(),
            category: "Applications".into(),
            label: "Discord Cache".into(),
            paths: vec![
                "~/Library/Application Support/discord/Cache".into(),
                "~/Library/Application Support/discord/Code Cache".into(),
                "~/Library/Application Support/discord/GPUCache".into(),
            ],
        },
        CleanRule {
            id: "slack_cache".into(),
            category: "Applications".into(),
            label: "Slack Cache".into(),
            paths: vec![
                "~/Library/Application Support/Slack/Cache".into(),
                "~/Library/Application Support/Slack/Code Cache".into(),
                "~/Library/Application Support/Slack/GPUCache".into(),
                "~/Library/Caches/com.tinyspeck.slackmacgap".into(),
            ],
        },
        CleanRule {
            id: "zoom_cache".into(),
            category: "Applications".into(),
            label: "Zoom Cache".into(),
            paths: vec!["~/Library/Caches/us.zoom.xos".into()],
        },
        CleanRule {
            id: "teams_cache".into(),
            category: "Applications".into(),
            label: "Teams Cache".into(),
            paths: vec![
                "~/Library/Caches/com.microsoft.teams2".into(),
                "~/Library/Application Support/Microsoft/Teams/Cache".into(),
            ],
        },
        CleanRule {
            id: "spotify_cache".into(),
            category: "Applications".into(),
            label: "Spotify Cache".into(),
            paths: vec![
                "~/Library/Caches/com.spotify.client".into(),
                "~/Library/Application Support/Spotify/PersistentCache".into(),
            ],
        },
        CleanRule {
            id: "vscode_cache".into(),
            category: "Applications".into(),
            label: "VS Code Cache".into(),
            paths: vec![
                "~/Library/Caches/com.microsoft.VSCode".into(),
                "~/Library/Application Support/Code/Cache".into(),
                "~/Library/Application Support/Code/CachedData".into(),
                "~/Library/Application Support/Code/CachedExtensions".into(),
            ],
        },
        CleanRule {
            id: "jetbrains_cache".into(),
            category: "Applications".into(),
            label: "JetBrains IDE Caches".into(),
            paths: vec!["~/Library/Caches/JetBrains".into()],
        },
        CleanRule {
            id: "notion_cache".into(),
            category: "Applications".into(),
            label: "Notion Cache".into(),
            paths: vec![
                "~/Library/Caches/notion.id".into(),
                "~/Library/Application Support/Notion/Cache".into(),
            ],
        },
    ]
}
