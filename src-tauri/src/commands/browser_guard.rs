//! Last line of defence against deleting web-browser profile data (open
//! tabs, sessions, passwords, cookies, extensions, history, bookmarks).
//!
//! Every module that removes files asks this guard first. Inside a known
//! browser root only the regenerable cache leaves in `CACHE_LEAVES` may go;
//! the root itself, anything else inside it, and any folder that contains a
//! root are refused. Browser caches under `~/Library/Caches/<browser>` are
//! not browser roots and stay cleanable.

use std::fmt;
use std::fs;
use std::path::{Component, Path};

/// Prefix of every refusal message, so the frontend can recognise it.
pub const ERROR_CODE: &str = "browser_profile_data";

#[derive(Clone, Copy, Debug)]
pub enum DeleteContext<'a> {
    /// Cleaner, optimizer, guardian, pruner, installers, analyzer.
    General,
    /// Uninstalling the app with this bundle id: that app's own browser
    /// data may go, but only when the root's owner matches exactly.
    Uninstall { bundle_id: &'a str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub path: String,
    pub reason: &'static str,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", ERROR_CODE, self.reason, self.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    NotBrowser,
    CacheLeaf,
    Protected {
        reason: &'static str,
        owner: Option<String>,
    },
}

enum Seg {
    Lit(&'static str),
    Prefix(&'static str),
    Any,
}

type Root = (&'static [Seg], Option<&'static str>);

use Seg::{Any, Lit, Prefix};

/// Browser roots relative to a `Library` folder, most specific first.
const NAMED_ROOTS: &[Root] = &[
    (&[Lit("Safari")], None),
    (&[Lit("Cookies"), Lit("Cookies.binarycookies")], None),
    (&[Lit("Application Support"), Lit("Google"), Lit("Chrome")], Some("com.google.Chrome")),
    (&[Lit("Application Support"), Lit("Google"), Lit("Chrome Beta")], Some("com.google.Chrome.beta")),
    (&[Lit("Application Support"), Lit("Google"), Lit("Chrome Dev")], Some("com.google.Chrome.dev")),
    (&[Lit("Application Support"), Lit("Google"), Lit("Chrome Canary")], Some("com.google.Chrome.canary")),
    (&[Lit("Application Support"), Lit("Google"), Lit("Chrome for Testing")], Some("com.google.chrome.for.testing")),
    (&[Lit("Application Support"), Lit("Google"), Prefix("Chrome")], None),
    (&[Lit("Application Support"), Lit("Microsoft Edge")], Some("com.microsoft.edgemac")),
    (&[Lit("Application Support"), Lit("Microsoft Edge Beta")], Some("com.microsoft.edgemac.Beta")),
    (&[Lit("Application Support"), Lit("Microsoft Edge Dev")], Some("com.microsoft.edgemac.Dev")),
    (&[Lit("Application Support"), Lit("Microsoft Edge Canary")], Some("com.microsoft.edgemac.Canary")),
    (&[Lit("Application Support"), Prefix("Microsoft Edge")], None),
    (&[Lit("Application Support"), Lit("BraveSoftware"), Lit("Brave-Browser")], Some("com.brave.Browser")),
    (&[Lit("Application Support"), Lit("BraveSoftware"), Lit("Brave-Browser-Beta")], Some("com.brave.Browser.beta")),
    (&[Lit("Application Support"), Lit("BraveSoftware"), Lit("Brave-Browser-Nightly")], Some("com.brave.Browser.nightly")),
    (&[Lit("Application Support"), Lit("BraveSoftware"), Any], None),
    (&[Lit("Application Support"), Lit("Arc")], Some("company.thebrowser.Browser")),
    (&[Lit("Application Support"), Lit("Dia")], Some("company.thebrowser.dia")),
    (&[Lit("Application Support"), Lit("Vivaldi")], Some("com.vivaldi.Vivaldi")),
    (&[Lit("Application Support"), Prefix("Vivaldi")], None),
    (&[Lit("Application Support"), Lit("Chromium")], Some("org.chromium.Chromium")),
    (&[Lit("Application Support"), Lit("Comet")], Some("ai.perplexity.comet")),
    (&[Lit("Application Support"), Lit("Thorium")], Some("org.chromium.Thorium")),
    (&[Lit("Application Support"), Lit("Sidekick")], Some("com.pushplaylabs.sidekick")),
    (&[Lit("Application Support"), Lit("SigmaOS")], None),
    (&[Lit("Application Support"), Lit("Yandex"), Lit("YandexBrowser")], Some("ru.yandex.desktop.yandex-browser")),
    (&[Lit("Application Support"), Lit("Firefox")], Some("org.mozilla.firefox")),
    (&[Lit("Application Support"), Lit("Mozilla")], None),
    (&[Lit("Application Support"), Lit("Waterfox")], Some("net.waterfox.waterfox")),
    (&[Lit("Application Support"), Lit("librewolf")], None),
    (&[Lit("Application Support"), Lit("Floorp")], None),
    (&[Lit("Application Support"), Lit("zen")], Some("app.zen-browser.zen")),
    (&[Lit("Application Support"), Lit("Mullvad Browser")], Some("net.mullvad.mullvadbrowser")),
    (&[Lit("Application Support"), Lit("TorBrowser-Data")], Some("org.torproject.torbrowser")),
    (&[Lit("Application Support"), Lit("Orion")], Some("com.kagi.kagimacOS")),
    (&[Lit("Application Support"), Lit("DuckDuckGo")], None),
];

/// `Library` subfolders whose entries are named after the owning bundle id.
const BUNDLE_LOCATIONS: &[&str] = &[
    "Application Support",
    "Application Scripts",
    "Containers",
    "Group Containers",
    "WebKit",
    "HTTPStorages",
    "Cookies",
    "Saved Application State",
    "Preferences",
];

/// Lowercase bundle-id prefixes of browsers (all channels).
pub const BROWSER_BUNDLE_PREFIXES: &[&str] = &[
    "com.google.chrome",
    "org.chromium.",
    "com.microsoft.edgemac",
    "com.brave.browser",
    "company.thebrowser.",
    "com.vivaldi.vivaldi",
    "com.operasoftware.",
    "net.imput.helium",
    "ai.perplexity.comet",
    "com.openai.atlas",
    "com.pushplaylabs.sidekick",
    "io.island.",
    "com.sigmaos.",
    "ru.yandex.desktop.yandex-browser",
    "org.mozilla.firefox",
    "org.mozilla.nightly",
    "org.mozilla.librewolf",
    "io.gitlab.librewolf",
    "net.waterfox.",
    "org.ablaze.floorp",
    "one.ablaze.floorp",
    "app.zen-browser.",
    "net.mullvad.mullvadbrowser",
    "org.torproject.",
    "com.apple.safari",
    "com.kagi.kagimacos",
    "com.duckduckgo.macos.browser",
];

/// Regenerable cache leaves, relative to a profile (or the browser root).
/// `Service Worker/CacheStorage` is deliberately absent: it holds PWA
/// offline data.
const CACHE_LEAVES: &[&[&str]] = &[
    &["Cache"],
    &["Code Cache"],
    &["GPUCache"],
    &["ShaderCache"],
    &["GrShaderCache"],
    &["GraphiteDawnCache"],
    &["DawnCache"],
    &["DawnGraphiteCache"],
    &["DawnWebGPUCache"],
    &["component_crx_cache"],
    &["extensions_crx_cache"],
    &["Media Cache"],
    &["cache2"],
    &["startupCache"],
    &["jumpListCache"],
    &["thumbnails"],
    &["Crashpad", "completed"],
    &["Data", "Library", "Caches"],
];

/// A folder holding any of these is a browser (or browser-like) profile.
const MARKER_FILES: &[&str] = &[
    "Local State",
    "profiles.ini",
    "Cookies.binarycookies",
    "Login Data",
    "key4.db",
    "logins.json",
    "places.sqlite",
    "cookies.sqlite",
];

/// Files that are browser credentials/state wherever they sit.
const PROFILE_FILE_NAMES: &[&str] = &[
    "Local State",
    "profiles.ini",
    "Cookies.binarycookies",
    "Login Data",
    "Login Data For Account",
    "Web Data",
    "key4.db",
    "logins.json",
    "places.sqlite",
    "cookies.sqlite",
];

/// Library children whose presence means a bare `Library` folder holds
/// browser data. Keeps unrelated `Library` folders (e.g. Unity projects)
/// deletable.
const LIBRARY_BROWSER_HOMES: &[&str] = &[
    "Safari",
    "Application Support",
    "Containers",
    "Group Containers",
    "WebKit",
    "HTTPStorages",
    "Cookies",
];

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn seg_matches(seg: &Seg, name: &str) -> bool {
    match seg {
        Lit(l) => eq(l, name),
        Prefix(p) => name.len() >= p.len() && name.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()),
        Any => true,
    }
}

/// Bundle id an entry in a `BUNDLE_LOCATIONS` folder belongs to: drops the
/// `.savedState` / `.binarycookies` / `.plist` suffix and a team-id or
/// `group.` prefix.
pub fn entry_bundle_id(name: &str) -> String {
    let mut s = name.to_string();
    for suffix in [".savedstate", ".binarycookies", ".plist"] {
        if s.len() > suffix.len() && s.to_ascii_lowercase().ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
            break;
        }
    }
    if let Some((first, rest)) = s.split_once('.') {
        if first.len() == 10 && first.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            s = rest.to_string();
        }
    }
    if s.len() > 6 && s[..6].eq_ignore_ascii_case("group.") {
        s = s[6..].to_string();
    }
    s
}

/// True if a bundle id (or a Library entry named after one) is a browser's.
pub fn is_browser_bundle_id(name: &str) -> bool {
    let id = entry_bundle_id(name).to_ascii_lowercase();
    BROWSER_BUNDLE_PREFIXES.iter().any(|p| id.starts_with(p))
}

fn components(path: &Path) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => out.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => return None,
            _ => {}
        }
    }
    Some(out)
}

/// Length and owner of the browser root that `rest` (components after a
/// `Library` folder) starts with.
fn match_root(rest: &[String]) -> Option<(usize, Option<String>)> {
    for (segs, owner) in NAMED_ROOTS {
        if rest.len() >= segs.len() && segs.iter().zip(rest).all(|(s, n)| seg_matches(s, n)) {
            return Some((segs.len(), owner.map(str::to_string)));
        }
    }
    if rest.len() >= 2
        && BUNDLE_LOCATIONS.iter().any(|l| eq(l, &rest[0]))
        && is_browser_bundle_id(&rest[1])
    {
        return Some((2, Some(entry_bundle_id(&rest[1]))));
    }
    None
}

/// True if `rest` names a folder that can contain a browser root.
fn is_root_ancestor(rest: &[String]) -> bool {
    if rest.len() == 1 && BUNDLE_LOCATIONS.iter().any(|l| eq(l, &rest[0])) {
        return true;
    }
    NAMED_ROOTS.iter().any(|(segs, _)| {
        rest.len() < segs.len() && segs.iter().zip(rest).all(|(s, n)| seg_matches(s, n))
    })
}

fn is_profile_dir_name(prefix: &[String], i: usize) -> bool {
    let c = prefix[i].to_ascii_lowercase();
    c == "default"
        || c == "user data"
        || c == "guest profile"
        || c == "system profile"
        || c == "profiles"
        || c.starts_with("profile ")
        || (i > 0 && prefix[i - 1].eq_ignore_ascii_case("profiles"))
}

/// True if `rel` (path inside a browser root) is at or under a cache leaf
/// reached only through profile folders.
fn is_allowlisted_cache(rel: &[String]) -> bool {
    for end in 0..rel.len() {
        for leaf in CACHE_LEAVES {
            let k = leaf.len();
            if end + 1 < k {
                continue;
            }
            let start = end + 1 - k;
            if leaf.iter().zip(&rel[start..=end]).all(|(l, n)| eq(l, n)) {
                let prefix = &rel[..start];
                return prefix.len() <= 3 && (0..prefix.len()).all(|i| is_profile_dir_name(prefix, i));
            }
        }
    }
    false
}

/// Path-only classification: no filesystem access, so rule paths can be
/// checked in tests.
pub fn static_verdict(path: &Path) -> Verdict {
    let Some(comps) = components(path) else {
        return Verdict::Protected { reason: "unnormalised path", owner: None };
    };
    for (i, c) in comps.iter().enumerate() {
        if !eq(c, "Library") {
            continue;
        }
        let rest = &comps[i + 1..];
        if let Some((len, owner)) = match_root(rest) {
            let rel = &rest[len..];
            if !rel.is_empty() && is_allowlisted_cache(rel) {
                return Verdict::CacheLeaf;
            }
            let reason = if rel.is_empty() { "browser profile root" } else { "inside a browser profile" };
            return Verdict::Protected { reason, owner };
        }
        if !rest.is_empty() && is_root_ancestor(rest) {
            return Verdict::Protected { reason: "contains browser profiles", owner: None };
        }
    }
    Verdict::NotBrowser
}

fn dir_has_marker(dir: &Path) -> bool {
    MARKER_FILES.iter().any(|m| dir.join(m).exists())
        || dir.join("WebsiteData").is_dir()
        || dir.join("Default/Preferences").is_file()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// True if `dir` or any folder up to `max_depth` levels below it looks like
/// a browser profile. Gives up (returning true) past `MARKER_SCAN_LIMIT`
/// folders, so an unreadably large tree is treated as protected.
pub fn contains_profile_markers(dir: &Path, max_depth: usize) -> bool {
    const MARKER_SCAN_LIMIT: usize = 4_000;
    if !is_real_dir(dir) {
        return false;
    }
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((d, depth)) = stack.pop() {
        visited += 1;
        if visited > MARKER_SCAN_LIMIT {
            return true;
        }
        if dir_has_marker(&d) {
            return true;
        }
        if depth >= max_depth {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&d) {
            for e in entries.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push((e.path(), depth + 1));
                }
            }
        }
    }
    false
}

/// Markers only full browsers leave (Electron apps have `Local State` too,
/// so it is not enough to lock down everything below it).
fn is_strong_profile_dir(d: &Path) -> bool {
    let has_credentials =
        |x: &Path| ["Login Data", "profiles.ini", "key4.db", "logins.json"].iter().any(|m| x.join(m).is_file());
    has_credentials(d)
        || has_credentials(&d.join("Default"))
        || d.join("Default/Preferences").is_file()
        || (d.file_name().is_some_and(|n| eq(&n.to_string_lossy(), "Default")) && d.join("Preferences").is_file())
}

/// True if `path` lies inside an unlisted browser's profile and is not one
/// of its cache leaves.
fn inside_unknown_profile(path: &Path) -> bool {
    for anc in path.ancestors().skip(1).take(6) {
        if is_strong_profile_dir(anc) {
            let rel = path.strip_prefix(anc).ok().and_then(components).unwrap_or_default();
            return !is_allowlisted_cache(&rel);
        }
    }
    false
}

fn library_holds_browser_data(library: &Path) -> bool {
    LIBRARY_BROWSER_HOMES.iter().any(|c| library.join(c).exists())
}

/// Filesystem-aware verdict: `static_verdict`, plus bare `Library` folders
/// (and folders holding one) that actually contain browser data.
fn verdict(path: &Path) -> Verdict {
    let v = static_verdict(path);
    if v != Verdict::NotBrowser {
        return v;
    }
    let ends_in_library = path.file_name().map(|n| eq(&n.to_string_lossy(), "Library")).unwrap_or(false);
    if (ends_in_library && library_holds_browser_data(path)) || library_holds_browser_data(&path.join("Library")) {
        return Verdict::Protected { reason: "contains browser profiles", owner: None };
    }
    Verdict::NotBrowser
}

fn refuse(path: &Path, reason: &'static str) -> Result<(), Refusal> {
    Err(Refusal { path: path.to_string_lossy().into_owned(), reason })
}

fn check_one(path: &Path, ctx: DeleteContext) -> Result<(), Refusal> {
    match verdict(path) {
        Verdict::CacheLeaf => return Ok(()),
        Verdict::Protected { reason, owner } => {
            if let DeleteContext::Uninstall { bundle_id } = ctx {
                if !bundle_id.is_empty() && owner.as_deref().is_some_and(|o| eq(o, bundle_id)) {
                    return Ok(());
                }
            }
            return refuse(path, reason);
        }
        Verdict::NotBrowser => {}
    }
    match ctx {
        DeleteContext::General => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if PROFILE_FILE_NAMES.iter().any(|f| eq(f, &name)) {
                return refuse(path, "browser credential or state file");
            }
            if contains_profile_markers(path, 1) {
                return refuse(path, "looks like a browser profile");
            }
            if inside_unknown_profile(path) {
                return refuse(path, "inside a browser profile");
            }
        }
        DeleteContext::Uninstall { bundle_id } => {
            // Electron apps keep `Local State` too, so only full-browser
            // markers count here, and an entry named after the app is its own.
            let owned = !bundle_id.is_empty()
                && library_entry_owner(path).is_some_and(|o| eq(&o, bundle_id));
            if !owned && (is_strong_profile_dir(path) || inside_unknown_profile(path)) {
                return refuse(path, "looks like a browser profile");
            }
        }
    }
    Ok(())
}

/// Bundle id of the `Library/<location>/<entry>` that `path` sits in.
fn library_entry_owner(path: &Path) -> Option<String> {
    let comps = components(path)?;
    (0..comps.len().saturating_sub(2)).find_map(|i| {
        (eq(&comps[i], "Library") && BUNDLE_LOCATIONS.iter().any(|l| eq(l, &comps[i + 1])))
            .then(|| entry_bundle_id(&comps[i + 2]))
    })
}

/// Decides whether `path` may be deleted. Checks the literal path and, if it
/// resolves elsewhere, the resolved one too.
pub fn check(path: &Path, ctx: DeleteContext) -> Result<(), Refusal> {
    check_one(path, ctx)?;
    if let Ok(canonical) = fs::canonicalize(path) {
        if canonical != path {
            check_one(&canonical, ctx).map_err(|mut r| {
                r.path = path.to_string_lossy().into_owned();
                r
            })?;
        }
    }
    Ok(())
}

pub fn check_general(path: &Path) -> Result<(), Refusal> {
    check(path, DeleteContext::General)
}

#[cfg(test)]
pub mod fixtures {
    //! A fake home holding realistic browser profiles, for tests that run
    //! real delete code against it.

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::commands::test_support::set_age_days;

    pub struct FakeBrowsers {
        pub home: PathBuf,
        /// Every profile-data file, with its original contents.
        pub protected: BTreeMap<PathBuf, Vec<u8>>,
        /// Regenerable cache files the cleaner is allowed to remove.
        pub cache_leaves: Vec<PathBuf>,
        /// Browser roots, profile dirs and their parents.
        pub protected_dirs: Vec<PathBuf>,
    }

    impl FakeBrowsers {
        /// Panics naming every profile file that was removed or changed.
        pub fn assert_profiles_intact(&self) {
            let damaged: Vec<String> = self
                .protected
                .iter()
                .filter(|(p, bytes)| fs::read(p).ok().as_deref() != Some(bytes.as_slice()))
                .map(|(p, _)| p.strip_prefix(&self.home).unwrap_or(p).display().to_string())
                .collect();
            assert!(damaged.is_empty(), "browser profile data damaged: {damaged:#?}");
        }
    }

    const CHROMIUM_PROFILE_DATA: &[&str] = &[
        "Sessions/Session_13370000000000000",
        "Sessions/Tabs_13370000000000000",
        "Current Session",
        "Current Tabs",
        "Session Storage/000003.log",
        "Local Storage/leveldb/000003.log",
        "IndexedDB/https_mail.example.com_0.indexeddb.leveldb/000003.log",
        "Service Worker/CacheStorage/5f1e0a/index.txt",
        "Service Worker/ScriptCache/index",
        "Extensions/aeblfdkhhhdcdjpifhhbdiojplfjncoa/8.10.0_0/manifest.json",
        "Local Extension Settings/aeblfdkhhhdcdjpifhhbdiojplfjncoa/000003.log",
        "Sync Extension Settings/aeblfdkhhhdcdjpifhhbdiojplfjncoa/000003.log",
        "Extension State/000003.log",
        "Login Data",
        "Login Data For Account",
        "Cookies",
        "Web Data",
        "History",
        "Bookmarks",
        "Preferences",
        "Secure Preferences",
        "Storage/ext/abc/def/GPUCache/data_0",
    ];

    const CHROMIUM_PROFILE_CACHES: &[&str] = &[
        "Cache/Cache_Data/f_000001",
        "Code Cache/js/index",
        "GPUCache/data_0",
    ];

    const CHROMIUM_ROOT_CACHES: &[&str] = &[
        "ShaderCache/data_0",
        "GrShaderCache/data_0",
        "GraphiteDawnCache/data_0",
        "component_crx_cache/abc.crx",
        "extensions_crx_cache/abc.crx",
        "Crashpad/completed/report.dmp",
    ];

    /// (user-data dir relative to Application Support, extra root files)
    const CHROMIUM_BROWSERS: &[(&str, &[&str])] = &[
        ("Google/Chrome", &["Local State", "Crashpad/settings.dat"]),
        ("Google/Chrome Canary", &["Local State"]),
        ("Arc/User Data", &["Local State"]),
        ("Dia/User Data", &["Local State"]),
        ("BraveSoftware/Brave-Browser", &["Local State"]),
        ("Microsoft Edge", &["Local State"]),
        ("net.imput.helium", &["Local State"]),
        ("Comet", &["Local State"]),
        ("Vivaldi", &["Local State"]),
        ("com.operasoftware.Opera", &["Local State"]),
    ];

    const FIREFOX_PROFILE_DATA: &[&str] = &[
        "logins.json",
        "key4.db",
        "places.sqlite",
        "cookies.sqlite",
        "sessionstore.jsonlz4",
        "sessionstore-backups/recovery.jsonlz4",
        "storage/default/https+++mail.example.com/idb/1.sqlite",
        "extensions/uBlock0@raymondhill.net.xpi",
        "prefs.js",
    ];

    const FIREFOX_PROFILE_CACHES: &[&str] = &["cache2/entries/ABC", "startupCache/scriptCache.bin"];

    const OTHER_PROFILE_DATA: &[&str] = &[
        "Library/Application Support/Arc/StorableSidebar.json",
        "Library/Safari/History.db",
        "Library/Safari/Bookmarks.plist",
        "Library/Safari/LastSession.plist",
        "Library/Containers/com.apple.Safari/Data/Library/Safari/History.db",
        "Library/Containers/com.apple.Safari/Data/Library/WebKit/WebsiteData/Default/salt/LocalStorage/x.sqlite3",
        "Library/Containers/com.apple.Safari/Data/tmp/session.tmp",
        "Library/Containers/com.duckduckgo.macos.browser/Data/Library/WebKit/WebsiteData/IndexedDB/x",
        "Library/WebKit/com.apple.Safari/WebsiteData/LocalStorage/https_example.com_0.localstorage",
        "Library/WebKit/com.kagi.kagimacOS/WebsiteData/IndexedDB/x",
        "Library/HTTPStorages/com.apple.Safari/httpstorages.sqlite",
        "Library/HTTPStorages/com.kagi.kagimacOS/httpstorages.sqlite",
        "Library/Cookies/Cookies.binarycookies",
        "Library/Cookies/com.kagi.kagimacOS.binarycookies",
        "Library/Application Support/Orion/Defaults/favorites.plist",
        "Library/Group Containers/S6N382Y83G.company.thebrowser.Browser/spaces.json",
        "Library/Saved Application State/com.apple.Safari.savedState/windows.plist",
        "Library/Saved Application State/com.google.Chrome.savedState/data.data",
        "Library/Saved Application State/company.thebrowser.Browser.savedState/data.data",
        "Library/Preferences/com.google.Chrome.plist",
        "Library/Preferences/company.thebrowser.Browser.plist",
        "Library/Application Support/SomeNewBrowser/Local State",
        "Library/Application Support/SomeNewBrowser/Default/Preferences",
        "Library/Application Support/SomeNewBrowser/Default/Login Data",
        "Library/Application Support/com.example.newbrowser/Default/Preferences",
        "Library/Application Support/com.example.newbrowser/Default/History",
    ];

    const OTHER_CACHES: &[&str] = &[
        "Library/Containers/com.apple.Safari/Data/Library/Caches/com.apple.Safari/Cache.db",
        "Library/Caches/Google/Chrome/Default/Cache/Cache_Data/f_000001",
        "Library/Caches/com.apple.Safari/Cache.db",
        "Library/Caches/Firefox/Profiles/abcd.default-release/cache2/entries/ABC",
        "Library/Caches/company.thebrowser.Browser/Cache.db",
    ];

    /// Top-level entries aged past every age gate (orphans, saved state).
    const AGED: &[&str] = &[
        "Library/Application Support/Google",
        "Library/Application Support/BraveSoftware",
        "Library/Application Support/Firefox",
        "Library/Application Support/zen",
        "Library/Application Support/Arc",
        "Library/Application Support/Dia",
        "Library/Application Support/net.imput.helium",
        "Library/Application Support/com.operasoftware.Opera",
        "Library/Application Support/Comet",
        "Library/Application Support/Orion",
        "Library/Application Support/SomeNewBrowser",
        "Library/Application Support/com.example.newbrowser",
        "Library/Containers/com.apple.Safari",
        "Library/Containers/com.duckduckgo.macos.browser",
        "Library/WebKit/com.apple.Safari",
        "Library/WebKit/com.kagi.kagimacOS",
        "Library/HTTPStorages/com.apple.Safari",
        "Library/HTTPStorages/com.kagi.kagimacOS",
        "Library/Cookies/com.kagi.kagimacOS.binarycookies",
        "Library/Group Containers/S6N382Y83G.company.thebrowser.Browser",
        "Library/Saved Application State/com.apple.Safari.savedState",
        "Library/Saved Application State/com.google.Chrome.savedState",
        "Library/Saved Application State/company.thebrowser.Browser.savedState",
        "Library/Preferences/com.google.Chrome.plist",
        "Library/Preferences/company.thebrowser.Browser.plist",
    ];

    fn put(home: &Path, rel: &str, tag: usize) -> (PathBuf, Vec<u8>) {
        let p = home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut bytes = format!("{rel}#{tag}\n").into_bytes();
        bytes.resize(4096, b'k');
        fs::write(&p, &bytes).unwrap();
        (p, bytes)
    }

    pub fn build(home: &Path) -> FakeBrowsers {
        let mut protected = BTreeMap::new();
        let mut cache_leaves = Vec::new();
        let mut protected_dirs = Vec::new();
        let support = "Library/Application Support";
        let mut n = 0;
        let mut data = |rel: String, protected: &mut BTreeMap<PathBuf, Vec<u8>>| {
            n += 1;
            let (p, b) = put(home, &rel, n);
            protected.insert(p, b);
        };

        for (root, root_files) in CHROMIUM_BROWSERS {
            let root = format!("{support}/{root}");
            for f in *root_files {
                data(format!("{root}/{f}"), &mut protected);
            }
            for profile in ["Default", "Profile 1"] {
                for f in CHROMIUM_PROFILE_DATA {
                    data(format!("{root}/{profile}/{f}"), &mut protected);
                }
                for f in CHROMIUM_PROFILE_CACHES {
                    cache_leaves.push(put(home, &format!("{root}/{profile}/{f}"), 0).0);
                }
                protected_dirs.push(home.join(format!("{root}/{profile}")));
            }
            for f in CHROMIUM_ROOT_CACHES {
                cache_leaves.push(put(home, &format!("{root}/{f}"), 0).0);
            }
            protected_dirs.push(home.join(&root));
        }
        for root in ["Firefox", "zen"] {
            let root = format!("{support}/{root}");
            data(format!("{root}/profiles.ini"), &mut protected);
            for profile in ["Profiles/abcd.default-release", "Profiles/efgh.dev-edition-default"] {
                for f in FIREFOX_PROFILE_DATA {
                    data(format!("{root}/{profile}/{f}"), &mut protected);
                }
                for f in FIREFOX_PROFILE_CACHES {
                    cache_leaves.push(put(home, &format!("{root}/{profile}/{f}"), 0).0);
                }
                protected_dirs.push(home.join(format!("{root}/{profile}")));
            }
            protected_dirs.push(home.join(&root));
        }
        for rel in OTHER_PROFILE_DATA {
            data(rel.to_string(), &mut protected);
        }
        for rel in OTHER_CACHES {
            cache_leaves.push(put(home, rel, 0).0);
        }
        for rel in [
            "Library/Application Support/Google",
            "Library/Application Support/BraveSoftware",
            "Library/Application Support/Arc",
            "Library/Application Support/Dia",
            "Library/Application Support/Orion",
            "Library/Application Support/SomeNewBrowser",
            "Library/Application Support/com.example.newbrowser",
            "Library/Application Support",
            "Library/Safari",
            "Library/Containers",
            "Library/Containers/com.apple.Safari",
            "Library/Containers/com.apple.Safari/Data",
            "Library/Containers/com.duckduckgo.macos.browser",
            "Library/Group Containers",
            "Library/Group Containers/S6N382Y83G.company.thebrowser.Browser",
            "Library/WebKit",
            "Library/WebKit/com.apple.Safari",
            "Library/WebKit/com.kagi.kagimacOS",
            "Library/HTTPStorages",
            "Library/HTTPStorages/com.apple.Safari",
            "Library/Cookies",
            "Library/Saved Application State",
            "Library/Saved Application State/com.apple.Safari.savedState",
            "Library/Saved Application State/com.google.Chrome.savedState",
            "Library",
        ] {
            protected_dirs.push(home.join(rel));
        }
        protected_dirs.push(home.to_path_buf());

        for rel in AGED {
            set_age_days(&home.join(rel), 400);
        }
        FakeBrowsers { home: home.to_path_buf(), protected, cache_leaves, protected_dirs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{canon, s, workspace_tempdir, write_file};
    use std::path::PathBuf;

    const H: &str = "/Users/tester";

    fn v(rel: &str) -> Verdict {
        static_verdict(&Path::new(H).join(rel))
    }

    fn protected(rel: &str) -> bool {
        matches!(v(rel), Verdict::Protected { .. })
    }

    #[test]
    fn allowlisted_cache_leaves_inside_browser_roots_are_allowed() {
        for rel in [
            "Library/Application Support/Google/Chrome/Default/Cache",
            "Library/Application Support/Google/Chrome/Default/Cache/Cache_Data/f_000001",
            "Library/Application Support/Google/Chrome/Profile 3/Code Cache",
            "Library/Application Support/Google/Chrome/Default/GPUCache",
            "Library/Application Support/Google/Chrome/ShaderCache",
            "Library/Application Support/Google/Chrome/GrShaderCache",
            "Library/Application Support/Google/Chrome/GraphiteDawnCache",
            "Library/Application Support/Google/Chrome/component_crx_cache",
            "Library/Application Support/Google/Chrome/extensions_crx_cache",
            "Library/Application Support/Google/Chrome/Crashpad/completed",
            "Library/Application Support/Google/Chrome Beta/Default/Cache",
            "Library/Application Support/BraveSoftware/Brave-Browser/ShaderCache",
            "Library/Application Support/BraveSoftware/Brave-Browser-Nightly/Default/Cache",
            "Library/Application Support/Arc/User Data/Default/Cache",
            "Library/Application Support/Dia/User Data/Profile 1/Code Cache",
            "Library/Application Support/net.imput.helium/GrShaderCache",
            "Library/Application Support/Microsoft Edge/Default/Media Cache",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/cache2",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/startupCache",
            "Library/Application Support/zen/Profiles/x.Default (release)/thumbnails",
            "Library/Containers/com.apple.Safari/Data/Library/Caches",
            "Library/Containers/com.apple.Safari/Data/Library/Caches/com.apple.Safari/Cache.db",
            "library/application support/google/chrome/default/cache",
        ] {
            assert_eq!(v(rel), Verdict::CacheLeaf, "{rel}");
        }
    }

    #[test]
    fn profile_data_inside_browser_roots_is_refused() {
        for rel in [
            "Library/Application Support/Google/Chrome",
            "Library/Application Support/Google/Chrome/Default",
            "Library/Application Support/Google/Chrome/Local State",
            "Library/Application Support/Google/Chrome/Snapshots/120.0",
            "Library/Application Support/Google/Chrome/Default/Sessions",
            "Library/Application Support/Google/Chrome/Default/Current Session",
            "Library/Application Support/Google/Chrome/Default/Session Storage",
            "Library/Application Support/Google/Chrome/Default/Local Storage",
            "Library/Application Support/Google/Chrome/Default/IndexedDB",
            "Library/Application Support/Google/Chrome/Default/IndexedDB/x/Cache",
            "Library/Application Support/Google/Chrome/Default/Service Worker",
            "Library/Application Support/Google/Chrome/Default/Service Worker/CacheStorage",
            "Library/Application Support/Google/Chrome/Default/Service Worker/CacheStorage/abc",
            "Library/Application Support/Google/Chrome/Default/Extensions",
            "Library/Application Support/Google/Chrome/Default/Extensions/id/1.0/Cache",
            "Library/Application Support/Google/Chrome/Default/Local Extension Settings",
            "Library/Application Support/Google/Chrome/Default/Extension State",
            "Library/Application Support/Google/Chrome/Default/Sync Extension Settings",
            "Library/Application Support/Google/Chrome/Default/Login Data",
            "Library/Application Support/Google/Chrome/Default/Cookies",
            "Library/Application Support/Google/Chrome/Default/Web Data",
            "Library/Application Support/Google/Chrome/Default/History",
            "Library/Application Support/Google/Chrome/Default/Bookmarks",
            "Library/Application Support/Google/Chrome/Default/Preferences",
            "Library/Application Support/Google/Chrome/Default/Secure Preferences",
            "Library/Application Support/Google/Chrome/Default/Storage/ext/a/def/GPUCache",
            "Library/Application Support/Google/Chrome/Crashpad",
            "Library/Application Support/Google/Chrome SxS",
            "Library/Application Support/Microsoft Edge Beta/Default",
            "Library/Application Support/BraveSoftware/Brave-Browser-Origin/Default",
            "Library/Application Support/Arc",
            "Library/Application Support/Arc/StorableSidebar.json",
            "Library/Application Support/Arc/User Data",
            "Library/Application Support/Dia/User Data/Default/Sessions",
            "Library/Application Support/company.thebrowser.dia",
            "Library/Application Support/net.imput.helium/Default",
            "Library/Application Support/com.operasoftware.Opera/Cookies",
            "Library/Application Support/Comet/Default/Login Data",
            "Library/Application Support/Vivaldi Snapshot/Default",
            "Library/Application Support/Firefox",
            "Library/Application Support/Firefox/profiles.ini",
            "Library/Application Support/Firefox/Profiles",
            "Library/Application Support/Firefox/Profiles/abcd.default-release",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/logins.json",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/key4.db",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/places.sqlite",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/cookies.sqlite",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/sessionstore.jsonlz4",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/sessionstore-backups",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/storage",
            "Library/Application Support/Firefox/Profiles/abcd.default-release/storage/default/x/cache",
            "Library/Application Support/Waterfox/Profiles/x/places.sqlite",
            "Library/Application Support/librewolf/Profiles/x",
            "Library/Application Support/Floorp/Profiles/x",
            "Library/Application Support/zen/Profiles/x",
            "Library/Application Support/Mullvad Browser/Profiles/x",
            "Library/Application Support/TorBrowser-Data/Browser/x",
            "Library/Application Support/Orion/Defaults",
            "Library/Safari",
            "Library/Safari/History.db",
            "Library/Safari/LastSession.plist",
            "Library/Containers/com.apple.Safari",
            "Library/Containers/com.apple.Safari/Data",
            "Library/Containers/com.apple.Safari/Data/Library/Safari",
            "Library/Containers/com.apple.Safari/Data/Library/WebKit",
            "Library/Containers/com.apple.Safari/Data/tmp",
            "Library/Containers/com.apple.SafariTechnologyPreview",
            "Library/Containers/com.duckduckgo.macos.browser",
            "Library/Containers/company.thebrowser.Browser",
            "Library/Group Containers/S6N382Y83G.company.thebrowser.Browser",
            "Library/Group Containers/group.com.apple.Safari",
            "Library/WebKit/com.apple.Safari",
            "Library/WebKit/com.apple.Safari/WebsiteData/LocalStorage",
            "Library/WebKit/com.kagi.kagimacOS",
            "Library/HTTPStorages/com.apple.Safari",
            "Library/HTTPStorages/com.google.Chrome",
            "Library/Cookies/Cookies.binarycookies",
            "Library/Cookies/com.kagi.kagimacOS.binarycookies",
            "Library/Saved Application State/com.apple.Safari.savedState",
            "Library/Saved Application State/com.google.Chrome.savedState",
            "Library/Saved Application State/company.thebrowser.Browser.savedState",
            "Library/Saved Application State/org.mozilla.firefox.savedState",
            "Library/Saved Application State/app.zen-browser.zen.savedState",
            "Library/Preferences/com.google.Chrome.plist",
            "Library/Application Scripts/com.apple.Safari.SafeBrowsing",
            "Library/Containers/com.foo/Data/Library/Application Support/Google/Chrome/Default",
        ] {
            assert!(protected(rel), "{rel} -> {:?}", v(rel));
        }
    }

    #[test]
    fn folders_that_contain_browser_roots_are_refused() {
        for rel in [
            "Library/Application Support",
            "Library/Application Support/Google",
            "Library/Application Support/BraveSoftware",
            "Library/Application Support/Yandex",
            "Library/Containers",
            "Library/Group Containers",
            "Library/WebKit",
            "Library/HTTPStorages",
            "Library/Cookies",
            "Library/Saved Application State",
            "Library/Preferences",
        ] {
            assert_eq!(
                v(rel),
                Verdict::Protected { reason: "contains browser profiles", owner: None },
                "{rel}"
            );
        }
    }

    #[test]
    fn unrelated_paths_are_not_browser_data() {
        for rel in [
            "Library/Caches/Google/Chrome",
            "Library/Caches/com.apple.Safari",
            "Library/Caches/company.thebrowser.Browser",
            "Library/Caches/Firefox/Profiles/x/cache2",
            "Library/Application Support/Google/GoogleUpdater/crx_cache",
            "Library/Application Support/Google/DriveFS",
            "Library/Application Support/Slack/Cache",
            "Library/Application Support/Code/Service Worker/CacheStorage",
            "Library/Application Support/com.google.Keystone",
            "Library/Application Support/Arcade",
            "Library/Application Support/Diagnostics",
            "Library/Containers/com.apple.mail/Data/Library/Caches",
            "Library/Saved Application State/com.example.app.savedState",
            "Library/Preferences/com.google.Keystone.Agent.plist",
            "Library/Logs/DiagnosticReports",
            "Documents/Library/notes.txt",
        ] {
            assert_eq!(v(rel), Verdict::NotBrowser, "{rel}");
        }
    }

    #[test]
    fn bundle_ids_of_every_supported_browser_are_recognised() {
        for id in [
            "com.google.Chrome",
            "com.google.Chrome.beta",
            "com.google.Chrome.canary",
            "com.google.Chrome.dev",
            "com.microsoft.edgemac",
            "com.microsoft.edgemac.Dev",
            "com.brave.Browser",
            "com.brave.Browser.nightly",
            "company.thebrowser.Browser",
            "company.thebrowser.dia",
            "net.imput.helium",
            "ai.perplexity.comet",
            "app.zen-browser.zen",
            "com.kagi.kagimacOS",
            "com.vivaldi.Vivaldi",
            "com.operasoftware.Opera",
            "com.operasoftware.OperaGX",
            "org.chromium.Chromium",
            "org.chromium.Thorium",
            "org.mozilla.firefox",
            "org.mozilla.firefoxdeveloperedition",
            "org.mozilla.nightly",
            "com.apple.Safari",
            "com.apple.SafariTechnologyPreview",
            "com.pushplaylabs.sidekick",
            "io.island.Island",
            "com.sigmaos.sigmaos.macos",
            "net.waterfox.waterfox",
            "io.gitlab.librewolf-community",
            "org.ablaze.floorp",
            "net.mullvad.mullvadbrowser",
            "org.torproject.torbrowser",
            "com.duckduckgo.macos.browser",
            "com.google.Chrome.savedState",
            "S6N382Y83G.company.thebrowser.Browser",
            "group.com.apple.Safari",
            "com.kagi.kagimacOS.binarycookies",
        ] {
            assert!(is_browser_bundle_id(id), "{id}");
        }
        for id in ["com.google.Keystone", "com.google.drivefs", "com.microsoft.teams", "org.mozilla.thunderbird", "com.example.app"] {
            assert!(!is_browser_bundle_id(id), "{id}");
        }
    }

    #[test]
    fn entry_names_map_back_to_bundle_ids() {
        assert_eq!(entry_bundle_id("com.google.Chrome.savedState"), "com.google.Chrome");
        assert_eq!(entry_bundle_id("com.kagi.kagimacOS.binarycookies"), "com.kagi.kagimacOS");
        assert_eq!(entry_bundle_id("com.google.Chrome.plist"), "com.google.Chrome");
        assert_eq!(entry_bundle_id("S6N382Y83G.company.thebrowser.Browser"), "company.thebrowser.Browser");
        assert_eq!(entry_bundle_id("group.com.apple.Safari"), "com.apple.Safari");
        assert_eq!(entry_bundle_id("net.imput.helium"), "net.imput.helium");
    }

    #[test]
    fn uninstall_may_remove_only_the_exact_browsers_own_data() {
        let chrome = Path::new(H).join("Library/Application Support/Google/Chrome");
        let canary_state = Path::new(H).join("Library/Saved Application State/com.google.Chrome.canary.savedState");
        let arc = Path::new(H).join("Library/Application Support/Arc");
        let firefox = Path::new(H).join("Library/Application Support/Firefox");
        let google = Path::new(H).join("Library/Application Support/Google");
        let chrome_ctx = DeleteContext::Uninstall { bundle_id: "com.google.Chrome" };

        assert!(check(&chrome, chrome_ctx).is_ok());
        assert!(check(&chrome, DeleteContext::Uninstall { bundle_id: "com.google.chrome" }).is_ok());
        assert!(check(&chrome.join("Default/Login Data"), chrome_ctx).is_ok());
        assert!(check(&canary_state, chrome_ctx).is_err());
        assert!(check(&google, chrome_ctx).is_err(), "vendor folder holds other apps");
        assert!(check(&chrome, DeleteContext::Uninstall { bundle_id: "com.google.Chrome.beta" }).is_err());
        assert!(check(&chrome, DeleteContext::Uninstall { bundle_id: "com.google.Keystone" }).is_err());
        assert!(check(&chrome, DeleteContext::Uninstall { bundle_id: "" }).is_err());
        assert!(check(&arc, DeleteContext::Uninstall { bundle_id: "company.thebrowser.something-else" }).is_err());
        assert!(check(&arc, DeleteContext::Uninstall { bundle_id: "company.thebrowser.Browser" }).is_ok());
        assert!(check(&firefox, DeleteContext::Uninstall { bundle_id: "org.mozilla.nightly" }).is_err());
        assert!(check(&chrome, DeleteContext::General).is_err());
    }

    #[test]
    fn refusals_carry_the_error_code() {
        let err = check_general(&Path::new(H).join("Library/Safari")).unwrap_err();
        assert!(err.to_string().starts_with("browser_profile_data: "), "{err}");
        assert!(err.to_string().ends_with("/Users/tester/Library/Safari"));
    }

    #[test]
    fn unknown_browser_profiles_are_detected_by_markers() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let unknown = root.join("Library/Application Support/com.example.newbrowser");
        write_file(&unknown.join("Local State"), 10);
        write_file(&unknown.join("Default/Cache/x"), 10);
        let ff = root.join("Some/FirefoxFork");
        write_file(&ff.join("profiles.ini"), 10);
        let wk = root.join("Library/WebKit/com.example.webapp");
        write_file(&wk.join("WebsiteData/LocalStorage/x"), 10);
        let parent = root.join("Some");
        let plain = root.join("Plain");
        write_file(&plain.join("a/b.bin"), 10);

        for p in [&unknown, &ff, &wk, &parent] {
            assert!(check_general(p).is_err(), "{}", p.display());
        }
        assert!(check_general(&unknown.join("Default/Cache")).is_ok(), "caches of unknown apps stay cleanable");
        assert!(check_general(&unknown.join("Local State")).is_err());
        assert!(check_general(&plain).is_ok());
        assert!(check(&unknown, DeleteContext::Uninstall { bundle_id: "com.example.newbrowser" }).is_ok());
        write_file(&unknown.join("Default/Login Data"), 10);
        assert!(check(&unknown, DeleteContext::Uninstall { bundle_id: "com.example.other" }).is_err());
        assert!(check(&unknown.join("Default"), DeleteContext::Uninstall { bundle_id: "com.example.other" }).is_err());
        assert!(check(&unknown, DeleteContext::Uninstall { bundle_id: "com.example.newbrowser" }).is_ok());
        let electron = root.join("Library/Application Support/Slack");
        write_file(&electron.join("Local State"), 10);
        write_file(&electron.join("Preferences"), 10);
        assert!(check(&electron, DeleteContext::Uninstall { bundle_id: "com.tinyspeck.slackmacgap" }).is_ok());
        assert!(contains_profile_markers(&root, 5));
        assert!(!contains_profile_markers(&plain, 5));
    }

    #[test]
    fn a_home_or_library_holding_browser_data_is_refused_but_other_library_folders_are_not() {
        let dir = workspace_tempdir();
        let home = canon(&dir).join("home");
        write_file(&home.join("Library/Safari/History.db"), 10);
        let unity = canon(&dir).join("MyGame/Library");
        write_file(&unity.join("ShaderCache/x"), 10);

        assert!(check_general(&home).is_err());
        assert!(check_general(&home.join("Library")).is_err());
        assert!(check_general(&unity).is_ok());
    }

    #[test]
    fn symlinks_into_browser_profiles_are_judged_by_their_target() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let profile = root.join("Library/Application Support/Google/Chrome/Default");
        write_file(&profile.join("Login Data"), 10);
        let link = root.join("innocent");
        std::os::unix::fs::symlink(&profile, &link).unwrap();
        assert!(check_general(&link).is_err());
    }

    #[test]
    fn traversal_is_refused() {
        assert!(protected("Library/Caches/../Safari"));
    }

    /// Every file-system path any cleaner rule can produce must either stay
    /// out of browser roots or land on an allowlisted cache leaf.
    #[test]
    fn no_cleaner_rule_reaches_into_browser_profiles() {
        let mut bad = Vec::new();
        for rule in crate::commands::cleaner::rules::all_rules() {
            for raw in &rule.paths {
                let path = PathBuf::from(raw.replacen('~', H, 1).replace('*', "x"));
                match static_verdict(&path) {
                    Verdict::NotBrowser | Verdict::CacheLeaf => {}
                    Verdict::Protected { reason: "contains browser profiles", .. } if rule.max_age_days.is_some() => {
                        // Age-filtered rules delete children one by one; the
                        // runtime guard filters browser children.
                    }
                    other => bad.push(format!("{} ({}): {:?}", rule.id, raw, other)),
                }
            }
        }
        assert!(bad.is_empty(), "rules reaching into browser profiles: {bad:#?}");
    }

    #[test]
    fn guard_names_in_tests_match_the_shared_fixture() {
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        for p in fx.protected.keys() {
            assert!(check_general(p).is_err(), "{} should be refused", s(p));
        }
        for p in &fx.protected_dirs {
            assert!(check_general(p).is_err(), "{} should be refused", s(p));
        }
        for p in &fx.cache_leaves {
            let leaf = p.parent().unwrap();
            assert!(check_general(leaf).is_ok(), "{} should be allowed", s(leaf));
        }
    }
}
