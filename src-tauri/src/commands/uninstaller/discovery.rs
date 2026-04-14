use std::fs;
use std::path::Path;

use super::brew;
use super::AppInfo;
use crate::commands::utils::path_size;

/// Patterns that indicate a data-sensitive app — password managers, crypto
/// wallets, 2FA authenticators, VPN clients, and encryption tools. Matched
/// as a case-insensitive substring against either the bundle identifier or
/// the display name. The UI surfaces a warning before uninstalling these
/// so users don't accidentally destroy irrecoverable credentials or keys.
const DATA_SENSITIVE_PATTERNS: &[&str] = &[
    // Password managers
    "1password",
    "agilebits",
    "bitwarden",
    "lastpass",
    "dashlane",
    "keepass",
    "keepassxc",
    "enpass",
    "nordpass",
    "keeper",
    "roboform",
    "padloc",
    "strongbox",
    "passky",
    "proton pass",
    "protonpass",
    // 2FA / authenticator apps
    "authy",
    "yubico",
    "yubikey",
    "raivo",
    "step two",
    "2fas",
    "tofu",
    "ente auth",
    // Crypto wallets (desktop)
    "metamask",
    "phantom",
    "exodus",
    "electrum",
    "ledger live",
    "trezor",
    "atomic wallet",
    "trust wallet",
    "coinbase wallet",
    "rainbow",
    "frame",
    "rabby",
    "keplr",
    "xdefi",
    "zerion",
    "monero",
    "bitcoin core",
    "litecoin",
    "wasabi wallet",
    "sparrow",
    // VPN clients
    "nordvpn",
    "expressvpn",
    "protonvpn",
    "surfshark",
    "windscribe",
    "mullvad",
    "privateinternetaccess",
    "private internet access",
    "tunnelblick",
    "wireguard",
    "openvpn",
    "tailscale",
    "zerotier",
    "cloudflare warp",
    "1.1.1.1",
    "hotspot shield",
    "ivpn",
    // Encryption & keys
    "veracrypt",
    "cryptomator",
    "boxcryptor",
    "gpgtools",
    "gpg suite",
    "pgp",
    "gpg",
    "ssh",
    "keychain",
    // SSH / terminal with saved credentials
    "termius",
    "royal tsx",
    "secure shellfish",
    "core tunnel",
];

/// Bundle ID prefixes for system-critical macOS services that must never
/// appear in the uninstall list. These are helpers, agents, and core OS
/// components that live outside /System/Applications/ but whose removal
/// would render the system unusable or unbootable.
const SYSTEM_CRITICAL_BUNDLES: &[&str] = &[
    // Core OS services — removal would break the system
    "com.apple.loginwindow",
    "com.apple.SecurityAgent",
    "com.apple.CoreServices",
    "com.apple.coreservices",
    "com.apple.backgroundtaskmanagement",
    "com.apple.SystemUIServer",
    "com.apple.WindowServer",
    "com.apple.Spotlight",
    "com.apple.dock",
    "com.apple.finder",
    "com.apple.notificationcenterui",
    "com.apple.controlcenter",
    "com.apple.AirPlayUIAgent",
    "com.apple.loginitems",
    "com.apple.sharedfilelist",
    "com.apple.sfl",
    "com.apple.metadata",
    "com.apple.security",
    "com.apple.keychain",
    "com.apple.trustd",
    "com.apple.securityd",
    "com.apple.frameworks",
    // System apps — built-in Apple applications
    "com.apple.Safari",
    "com.apple.mail",
    "com.apple.Terminal",
    "com.apple.Preview",
    "com.apple.TextEdit",
    "com.apple.Notes",
    "com.apple.reminders",
    "com.apple.iCal",
    "com.apple.AddressBook",
    "com.apple.Photos",
    "com.apple.AppStore",
    "com.apple.calculator",
    "com.apple.Dictionary",
    "com.apple.ActivityMonitor",
    "com.apple.Console",
    "com.apple.DiskUtility",
    "com.apple.KeychainAccess",
    "com.apple.FontBook",
    "com.apple.SystemProfiler",
    "com.apple.ScreenSharing",
    "com.apple.DigitalColorMeter",
    "com.apple.grapher",
    "com.apple.ScriptEditor2",
    "com.apple.VoiceOverUtility",
    "com.apple.BluetoothFileExchange",
    "com.apple.print.PrinterProxy",
    "com.apple.ColorSyncUtility",
    "com.apple.audio.AudioMIDISetup",
    "com.apple.DirectoryUtility",
    "com.apple.NetworkUtility",
    "com.apple.exposelauncher",
    "com.apple.MigrateAssistant",
    "com.apple.RAIDUtility",
    "com.apple.BootCampAssistant",
    "com.apple.Music",
    "com.apple.podcasts",
    "com.apple.iBooksX",
    "com.apple.iBooks",
    "com.apple.Automator",
    // System Settings variants
    "com.apple.systempreferences",
    "com.apple.SystemSettings",
    "com.apple.Settings",
    // Cloud and update services
    "com.apple.cloudd",
    "com.apple.iCloud",
    "com.apple.MobileSoftwareUpdate",
    "com.apple.SoftwareUpdate",
    "com.apple.installer",
    "com.apple.bird",
    "com.apple.CloudDocs",
    // Networking and connectivity
    "com.apple.WiFi",
    "com.apple.airport",
    "com.apple.Bluetooth",
    // Input methods
    "com.apple.inputmethod.",
    "com.apple.inputsource",
    "com.apple.TextInput",
    "com.apple.CharacterPicker",
    "com.apple.PressAndHold",
];

/// Returns true if the bundle ID belongs to a system-critical macOS service
/// that should be excluded from the uninstall list regardless of its path.
fn is_system_critical_bundle(bundle_id: &str) -> bool {
    if bundle_id.is_empty() {
        return false;
    }
    SYSTEM_CRITICAL_BUNDLES
        .iter()
        .any(|prefix| bundle_id.starts_with(prefix))
}

/// Returns true if the bundle_id or app name suggests a data-sensitive app.
fn check_data_sensitive(bundle_id: &str, name: &str) -> bool {
    let bid = bundle_id.to_lowercase();
    let n = name.to_lowercase();
    DATA_SENSITIVE_PATTERNS
        .iter()
        .any(|pat| bid.contains(pat) || n.contains(pat))
}

/// Reads an app bundle's Info.plist and extracts metadata.
fn read_app_info(app_path: &Path) -> Option<AppInfo> {
    let plist_path = app_path.join("Contents/Info.plist");
    let plist = plist::Value::from_file(&plist_path).ok()?;
    let dict = plist.as_dictionary()?;

    let bundle_id = dict
        .get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();

    let name = dict
        .get("CFBundleName")
        .or_else(|| dict.get("CFBundleDisplayName"))
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| {
            app_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
        .to_string();

    let version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();

    let size = path_size(app_path);

    let path_str = app_path.to_string_lossy().to_string();
    let is_system = path_str.starts_with("/System/Applications/");

    // Skip system-critical bundles that live outside /System/Applications/
    // but whose removal would break the OS (loginwindow, Finder, Dock, etc.).
    if !is_system && is_system_critical_bundle(&bundle_id) {
        return None;
    }

    let is_data_sensitive = check_data_sensitive(&bundle_id, &name);

    // Detect background-only / UI-element apps (helper agents with no
    // visible window). These declare LSBackgroundOnly or LSUIElement in
    // their Info.plist.
    let is_background = dict
        .get("LSBackgroundOnly")
        .and_then(|v| v.as_boolean())
        .unwrap_or(false);
    let is_ui_element = dict
        .get("LSUIElement")
        .and_then(|v| v.as_boolean())
        .unwrap_or(false);
    let is_background_only = is_background || is_ui_element;

    // Homebrew cask detection is skipped for system apps — they're never
    // brew-managed and the detection does disk I/O we don't need.
    let brew_cask = if is_system {
        None
    } else {
        brew::detect_cask(&path_str)
    };

    Some(AppInfo {
        bundle_id,
        name,
        version,
        path: path_str,
        size,
        is_system,
        is_data_sensitive,
        brew_cask,
        is_background_only,
    })
}

/// Returns true if the path is a nested .app inside another .app bundle.
/// For example, `Foo.app/Contents/Helpers/Bar.app` should be skipped because
/// `Bar.app` is an internal helper, not a user-visible application.
fn is_nested_app(path: &Path) -> bool {
    // Check if `.app/` appears anywhere before the final component.
    // The final component IS a .app, so we strip it and look for `.app/`
    // in the remaining prefix.
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        parent_str.contains(".app/") || parent_str.ends_with(".app")
    } else {
        false
    }
}

/// Scans a directory for .app bundles, skipping any that are nested inside
/// another .app bundle (e.g. helper apps under Foo.app/Contents/).
fn scan_dir(dir: &Path) -> Vec<AppInfo> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            p.extension().map(|ext| ext == "app").unwrap_or(false)
                && !is_nested_app(&p)
        })
        .filter_map(|e| read_app_info(&e.path()))
        .collect()
}

/// Scans a Homebrew Caskroom directory for .app bundles inside version subdirectories.
fn scan_caskroom(caskroom: &Path) -> Vec<AppInfo> {
    let entries = match fs::read_dir(caskroom) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };

    let mut apps = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let cask_dir = entry.path();
        if !cask_dir.is_dir() {
            continue;
        }
        // Each cask has version subdirectories, scan each for .app bundles
        let versions = match fs::read_dir(&cask_dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for version_entry in versions.filter_map(|e| e.ok()) {
            let version_dir = version_entry.path();
            if !version_dir.is_dir() {
                continue;
            }
            apps.extend(scan_dir(&version_dir));
        }
    }
    apps
}

/// Scans /Applications, ~/Applications, Homebrew Caskrooms, and Setapp
/// for installed apps. macOS built-in apps under /System/Applications are
/// excluded because they cannot be uninstalled.
pub fn scan_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    // User-installed applications
    apps.extend(scan_dir(Path::new("/Applications")));

    // Input Methods (system-level and user-level)
    apps.extend(scan_dir(Path::new("/Library/Input Methods")));

    // User applications
    if let Some(home) = dirs::home_dir() {
        apps.extend(scan_dir(&home.join("Library/Input Methods")));
        apps.extend(scan_dir(&home.join("Applications")));

        // Setapp applications
        let setapp_dir = home.join("Applications/Setapp");
        if setapp_dir.exists() {
            apps.extend(scan_dir(&setapp_dir));
        }
    }

    // External volumes: scan /Volumes/*/Applications for .app bundles,
    // skipping the boot volume (any volume whose Applications dir is the
    // same device+inode as /Applications or ~/Applications).
    if let Ok(volumes) = fs::read_dir("/Volumes") {
        let boot_apps = fs::metadata("/Applications").ok().map(|m| {
            use std::os::unix::fs::MetadataExt;
            (m.dev(), m.ino())
        });
        let home_apps = dirs::home_dir()
            .and_then(|h| fs::metadata(h.join("Applications")).ok())
            .map(|m| {
                use std::os::unix::fs::MetadataExt;
                (m.dev(), m.ino())
            });

        for vol_entry in volumes.filter_map(|e| e.ok()) {
            let vol_apps = vol_entry.path().join("Applications");
            if !vol_apps.is_dir() {
                continue;
            }
            // Skip if this is the same directory as the boot /Applications
            // or ~/Applications (same device+inode means same directory).
            if let Ok(meta) = fs::metadata(&vol_apps) {
                use std::os::unix::fs::MetadataExt;
                let id = (meta.dev(), meta.ino());
                if boot_apps == Some(id) || home_apps == Some(id) {
                    continue;
                }
            }
            apps.extend(scan_dir(&vol_apps));
        }
    }

    // Homebrew Caskroom locations
    for caskroom_path in &["/opt/homebrew/Caskroom", "/usr/local/Caskroom"] {
        let caskroom = Path::new(caskroom_path);
        if caskroom.exists() {
            apps.extend(scan_caskroom(caskroom));
        }
    }

    // Sort by name case-insensitively
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Deduplicate by path
    apps.dedup_by(|a, b| a.path == b.path);

    apps
}
