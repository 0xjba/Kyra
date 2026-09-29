//! Last line of defence against deleting irreplaceable user data.
//!
//! Browser profiles are classified by `browser_guard`; this module adds
//! passwords and keys, crypto wallets, message history, mail and PIM,
//! unsaved documents, media libraries, device backups, virtual machines,
//! databases, source repositories, notes, game saves and cloud-synced
//! folders. Every module that removes files asks `check` first.
//!
//! Inside a protected root only that root's regenerable cache leaves may
//! go; the root itself, anything else inside it, and any folder that
//! contains a root are refused. Cloud-synced folders are never deleted by
//! any context: a local delete syncs to every device.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::commands::browser_guard;

/// Prefix of every non-browser refusal message, so the frontend can
/// recognise it. Browser refusals keep `browser_guard::ERROR_CODE`.
pub const ERROR_CODE: &str = "protected_user_data";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Browser,
    Credentials,
    Wallet,
    Messages,
    MailPim,
    Documents,
    Media,
    DeviceBackup,
    VirtualMachine,
    Database,
    SourceCode,
    Notes,
    GameSaves,
    CloudSync,
    /// A folder holding roots of several categories (`~/Library`, a home).
    UserData,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Browser => "browser profile data",
            Category::Credentials => "passwords and keys",
            Category::Wallet => "crypto wallet data",
            Category::Messages => "message history",
            Category::MailPim => "mail, contacts, calendars and notes",
            Category::Documents => "unsaved documents",
            Category::Media => "media library",
            Category::DeviceBackup => "device backups",
            Category::VirtualMachine => "virtual machines",
            Category::Database => "database files",
            Category::SourceCode => "source code and developer archives",
            Category::Notes => "notes",
            Category::GameSaves => "game saves",
            Category::CloudSync => "cloud-synced files",
            Category::UserData => "user data",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DeleteContext<'a> {
    /// Cleaner, optimizer, Pawtrol autonomous clean, pruner, installers,
    /// analyzer without the user's explicit override.
    General,
    /// Uninstalling the app with this bundle id. Its own browser data may
    /// go (see `browser_guard`); every other protected category outlives
    /// the app and is kept.
    Uninstall { bundle_id: &'a str },
    /// The user confirmed this specific data-loss deletion (Pawtrol user
    /// data categories). Allows everything but cloud-synced folders and
    /// folders holding several kinds of user data.
    Confirmed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub path: String,
    pub category: Category,
    pub detail: &'static str,
    /// True when the path is refused only because it holds protected data
    /// further down, so its other children may still be deletable.
    pub contains: bool,
}

impl Refusal {
    fn from_browser(r: browser_guard::Refusal) -> Self {
        Refusal {
            contains: r.reason == "contains browser profiles",
            path: r.path,
            category: Category::Browser,
            detail: r.reason,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.category == Category::Browser {
            write!(f, "{}: {}: {}", browser_guard::ERROR_CODE, self.detail, self.path)
        } else {
            write!(f, "{}: {} ({}): {}", ERROR_CODE, self.category.label(), self.detail, self.path)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    NotProtected,
    CacheLeaf,
    Protected {
        category: Category,
        detail: &'static str,
        contains: bool,
    },
}

#[derive(Clone, Copy)]
enum Seg {
    Lit(&'static str),
    Prefix(&'static str),
    Suffix(&'static str),
    Any,
}

use Seg::{Any, Lit, Prefix, Suffix};

type Leaves = &'static [&'static [Seg]];

struct Root {
    segs: &'static [Seg],
    cat: Category,
    leaves: Leaves,
}

const fn root(segs: &'static [Seg], cat: Category, leaves: Leaves) -> Root {
    Root { segs, cat, leaves }
}

use Category::*;

const NONE: Leaves = &[];
const ELECTRON: Leaves = &[
    &[Lit("Cache")],
    &[Lit("Caches")],
    &[Lit("Code Cache")],
    &[Lit("GPUCache")],
    &[Lit("DawnCache")],
    &[Lit("DawnGraphiteCache")],
    &[Lit("DawnWebGPUCache")],
    &[Lit("GraphiteDawnCache")],
    &[Lit("ShaderCache")],
    &[Lit("GrShaderCache")],
    &[Lit("Crashpad"), Lit("completed")],
    &[Lit("logs")],
];
const TEAMS: Leaves = &[
    &[Lit("Cache")],
    &[Lit("Code Cache")],
    &[Lit("GPUCache")],
    &[Lit("Application Cache")],
    &[Lit("logs")],
    &[Lit("tmp")],
];
const CONTAINER: Leaves = &[&[Lit("Data"), Lit("Library"), Lit("Caches")]];
const OFFICE_CONTAINER: Leaves = &[
    &[Lit("Data"), Lit("Library"), Lit("Caches")],
    &[Lit("Data"), Lit("Library"), Lit("Logs")],
    &[Lit("Data"), Lit("tmp")],
];
const MAIL_CONTAINER: Leaves = &[
    &[Lit("Data"), Lit("Library"), Lit("Caches")],
    &[Lit("Data"), Lit("Library"), Lit("Mail Downloads")],
];
const GROUP: Leaves = &[&[Lit("Library"), Lit("Caches")]];
const MESSAGES: Leaves = &[&[Lit("Caches")], &[Lit("StickerCache")]];
const STEAM: Leaves = &[
    &[Lit("appcache")],
    &[Lit("htmlcache")],
    &[Lit("depotcache")],
    &[Lit("logs")],
    &[Lit("dumps")],
    &[Lit("steamapps"), Lit("shadercache")],
    &[Lit("config"), Lit("htmlcache")],
];
const MINECRAFT: Leaves = &[&[Lit("logs")], &[Lit("crash-reports")], &[Lit("webcache")], &[Lit("webcache2")]];
const EMULATOR: Leaves = &[&[Lit("cache")], &[Lit("logs")]];
const RESOLVE: Leaves = &[
    &[Lit("DaVinci Resolve"), Lit("Cache")],
    &[Lit("DaVinci Resolve"), Lit("CacheClip")],
];
const ADOBE: Leaves = &[
    &[Lit("Common"), Lit("Media Cache")],
    &[Lit("Common"), Lit("Media Cache Files")],
];
const ADDRESS_BOOK: Leaves = &[&[Lit("Sources"), Any, Lit("Photos.cache")]];
const XCODE_USER_DATA: Leaves = &[&[Lit("Documentation")], &[Lit("Previews")], &[Lit("IB Support")]];
const HOMEBREW_VAR: Leaves = &[&[Lit("log")], &[Lit("homebrew"), Lit("locks")]];
const LOGS: Leaves = &[&[Lit("logs")]];
const AWS: Leaves = &[&[Lit("cli"), Lit("cache")]];
const KUBE: Leaves = &[&[Lit("cache")], &[Lit("http-cache")]];
const DOCKER_HOME: Leaves = &[&[Lit("buildx"), Lit("cache")]];
const VIRTUALBOX_VMS: Leaves = &[&[Lit(".cache")]];

/// Roots relative to a `Library` folder (the user's, `/Library`, or one
/// nested in a sandbox container).
const LIBRARY_ROOTS: &[Root] = &[
    root(&[Lit("Keychains")], Credentials, NONE),
    root(&[Lit("Messages")], Messages, MESSAGES),
    root(&[Lit("Mail")], MailPim, NONE),
    root(&[Lit("Calendars")], MailPim, NONE),
    root(&[Lit("Reminders")], MailPim, NONE),
    root(&[Lit("Autosave Information")], Documents, NONE),
    root(&[Lit("Mobile Documents")], CloudSync, NONE),
    root(&[Lit("CloudStorage")], CloudSync, NONE),
    root(&[Lit("Dropbox")], CloudSync, NONE),
    root(&[Lit("VirtualBox")], VirtualMachine, NONE),
    root(&[Lit("Audio"), Lit("Presets")], Media, NONE),
    root(&[Lit("Developer"), Lit("Xcode"), Lit("Archives")], SourceCode, NONE),
    root(&[Lit("Developer"), Lit("Xcode"), Lit("UserData")], SourceCode, XCODE_USER_DATA),
    root(&[Lit("Application Support"), Lit("Microsoft"), Lit("Teams")], Messages, TEAMS),
    root(&[Lit("Application Support"), Lit("Microsoft"), Lit("Skype for Desktop")], Messages, ELECTRON),
    root(&[Lit("Application Support"), Lit("Google"), Lit("DriveFS")], CloudSync, NONE),
    root(&[Lit("Application Support"), Lit("JetBrains"), Any, Lit("scratches")], SourceCode, NONE),
    root(&[Lit("Application Support"), Lit("JetBrains"), Any, Lit("consoles")], SourceCode, NONE),
    root(&[Lit("Application Support"), Lit("Blackmagic Design")], Media, RESOLVE),
    root(&[Lit("Application Support"), Lit("Adobe")], Media, ADOBE),
    root(&[Lit("Application Support"), Lit("AddressBook")], MailPim, ADDRESS_BOOK),
    root(&[Lit("Application Support"), Lit("Steam")], GameSaves, STEAM),
    root(&[Lit("Application Support"), Lit("minecraft")], GameSaves, MINECRAFT),
    root(&[Lit("Application Support"), Lit("PCSX2")], GameSaves, EMULATOR),
    root(&[Lit("Application Support"), Lit("rpcs3")], GameSaves, EMULATOR),
    root(&[Lit("Application Support"), Prefix("unity.")], GameSaves, NONE),
];

const APP_SUPPORT: &[(Seg, Category, Leaves)] = &[
    (Lit("1Password"), Credentials, ELECTRON),
    (Lit("Bitwarden"), Credentials, ELECTRON),
    (Lit("KeePassXC"), Credentials, NONE),
    (Lit("Authy Desktop"), Credentials, ELECTRON),
    (Lit("Enpass"), Credentials, NONE),
    (Lit("Strongbox"), Credentials, NONE),
    (Lit("Proton Pass"), Credentials, ELECTRON),
    (Lit("LastPass"), Credentials, NONE),
    (Lit("Dashlane"), Credentials, NONE),
    (Lit("Ledger Live"), Wallet, ELECTRON),
    (Lit("Exodus"), Wallet, ELECTRON),
    (Lit("Electrum"), Wallet, NONE),
    (Lit("Bitcoin"), Wallet, NONE),
    (Lit("Litecoin"), Wallet, NONE),
    (Lit("Ethereum"), Wallet, NONE),
    (Lit("Trezor Suite"), Wallet, ELECTRON),
    (Lit("@trezor"), Wallet, NONE),
    (Lit("atomic"), Wallet, ELECTRON),
    (Lit("Atomic Wallet"), Wallet, ELECTRON),
    (Lit("Coinomi"), Wallet, NONE),
    (Lit("monero-project"), Wallet, NONE),
    (Lit("Monero"), Wallet, NONE),
    (Lit("Frame"), Wallet, ELECTRON),
    (Lit("Phantom"), Wallet, ELECTRON),
    (Lit("Rabby"), Wallet, ELECTRON),
    (Lit("Rabby Wallet"), Wallet, ELECTRON),
    (Lit("Sparrow"), Wallet, NONE),
    (Lit("WalletWasabi"), Wallet, NONE),
    (Lit("Wasabi Wallet"), Wallet, NONE),
    (Lit("Trust Wallet"), Wallet, ELECTRON),
    (Lit("Coinbase Wallet"), Wallet, ELECTRON),
    (Lit("Guarda"), Wallet, ELECTRON),
    (Lit("Specter"), Wallet, NONE),
    (Lit("Feather"), Wallet, NONE),
    (Lit("Zcash"), Wallet, NONE),
    (Lit("Keplr"), Wallet, ELECTRON),
    (Lit("Signal"), Messages, ELECTRON),
    (Lit("Signal Beta"), Messages, ELECTRON),
    (Lit("WhatsApp"), Messages, ELECTRON),
    (Lit("Telegram Desktop"), Messages, NONE),
    (Lit("Slack"), Messages, ELECTRON),
    (Lit("discord"), Messages, ELECTRON),
    (Lit("discordcanary"), Messages, ELECTRON),
    (Lit("discordptb"), Messages, ELECTRON),
    (Lit("legcord"), Messages, ELECTRON),
    (Lit("Element"), Messages, ELECTRON),
    (Lit("Wire"), Messages, ELECTRON),
    (Lit("Microsoft Teams"), Messages, TEAMS),
    (Lit("ViberPC"), Messages, NONE),
    (Lit("Viber"), Messages, NONE),
    (Lit("Threema"), Messages, ELECTRON),
    (Lit("Session"), Messages, ELECTRON),
    (Lit("Beeper"), Messages, ELECTRON),
    (Lit("Mattermost"), Messages, ELECTRON),
    (Lit("Rocket.Chat"), Messages, ELECTRON),
    (Lit("Zulip"), Messages, ELECTRON),
    (Lit("Keybase"), Messages, ELECTRON),
    (Lit("Ferdium"), Messages, ELECTRON),
    (Lit("Rambox"), Messages, ELECTRON),
    (Lit("Messenger"), Messages, ELECTRON),
    (Lit("Caprine"), Messages, ELECTRON),
    (Lit("CallHistoryDB"), MailPim, NONE),
    (Lit("Thunderbird"), MailPim, NONE),
    (Lit("Capture One"), Media, NONE),
    (Lit("Logic"), Media, NONE),
    (Lit("GarageBand"), Media, NONE),
    (Lit("Figma"), Media, ELECTRON),
    (Lit("Pixelmator Pro"), Media, NONE),
    (Lit("MobileSync"), DeviceBackup, NONE),
    (Lit("VirtualBuddy"), VirtualMachine, NONE),
    (Lit("CrossOver"), VirtualMachine, NONE),
    (Lit("Postgres"), Database, NONE),
    (Lit("DBngin"), Database, NONE),
    (Lit("obsidian"), Notes, ELECTRON),
    (Lit("Notion"), Notes, ELECTRON),
    (Lit("Logseq"), Notes, ELECTRON),
    (Lit("Evernote"), Notes, ELECTRON),
    (Lit("Joplin"), Notes, ELECTRON),
    (Lit("Standard Notes"), Notes, ELECTRON),
    (Lit("anytype"), Notes, ELECTRON),
    (Lit("Heptabase"), Notes, ELECTRON),
    (Lit("OpenEmu"), GameSaves, NONE),
    (Lit("Dolphin"), GameSaves, NONE),
    (Lit("Ryujinx"), GameSaves, NONE),
    (Lit("RetroArch"), GameSaves, NONE),
    (Lit("GOG.com"), GameSaves, NONE),
    (Lit("Epic"), GameSaves, NONE),
    (Lit("Feral Interactive"), GameSaves, NONE),
    (Lit("Aspyr"), GameSaves, NONE),
    (Lit("PrismLauncher"), GameSaves, NONE),
    (Lit("MultiMC"), GameSaves, NONE),
    (Lit("CloudDocs"), CloudSync, NONE),
    (Lit("FileProvider"), CloudSync, NONE),
];

/// Lowercase bundle-id prefixes of apps whose data outlives them. Matched
/// against entries of `DATA_BUNDLE_LOCATIONS` after the team-id and
/// `group.` prefixes are dropped. The last field overrides the default
/// container leaves.
const DATA_BUNDLES: &[(&str, Category, Option<Leaves>)] = &[
    ("com.1password", Credentials, None),
    ("com.agilebits", Credentials, None),
    ("com.bitwarden", Credentials, None),
    ("org.keepassxc", Credentials, None),
    ("com.keepassium", Credentials, None),
    ("com.markmcguill.strongbox", Credentials, None),
    ("in.sinew.enpass", Credentials, None),
    ("com.lastpass", Credentials, None),
    ("com.dashlane", Credentials, None),
    ("me.proton.pass", Credentials, None),
    ("com.authy", Credentials, None),
    ("com.twilio.authy", Credentials, None),
    ("com.2stable", Credentials, None),
    ("com.raivo", Credentials, None),
    ("com.ledger.live", Wallet, None),
    ("com.electron.exodus", Wallet, None),
    ("io.exodus", Wallet, None),
    ("org.electrum", Wallet, None),
    ("org.bitcoinfoundation", Wallet, None),
    ("org.bitcoin", Wallet, None),
    ("com.sparrowwallet", Wallet, None),
    ("io.trezor", Wallet, None),
    ("io.atomicwallet", Wallet, None),
    ("com.atomicwallet", Wallet, None),
    ("com.coinomi", Wallet, None),
    ("org.getmonero", Wallet, None),
    ("sh.frame", Wallet, None),
    ("app.phantom", Wallet, None),
    ("com.rabby", Wallet, None),
    ("io.rabby", Wallet, None),
    ("io.wasabiwallet", Wallet, None),
    ("com.trustwallet", Wallet, None),
    ("io.bluewallet", Wallet, None),
    ("org.whispersystems.signal", Messages, None),
    ("net.whatsapp", Messages, None),
    ("desktop.whatsapp", Messages, None),
    ("ru.keepcoder.telegram", Messages, None),
    ("org.telegram", Messages, None),
    ("com.tdesktop.telegram", Messages, None),
    ("com.tinyspeck.slackmacgap", Messages, None),
    ("com.hnc.discord", Messages, None),
    ("com.microsoft.teams", Messages, None),
    ("com.skype.skype", Messages, None),
    ("com.tencent.xinwechat", Messages, None),
    ("com.tencent.wechat", Messages, None),
    ("jp.naver.line", Messages, None),
    ("com.viber.osx", Messages, None),
    ("com.facebook.archon", Messages, None),
    ("im.riot", Messages, None),
    ("com.wire.", Messages, None),
    ("ch.threema", Messages, None),
    ("com.apple.mobilesms", Messages, None),
    ("com.apple.ichat", Messages, None),
    ("com.apple.mail", MailPim, Some(MAIL_CONTAINER)),
    ("com.apple.ical", MailPim, None),
    ("com.apple.calendar", MailPim, None),
    ("com.apple.addressbook", MailPim, None),
    ("com.apple.contacts", MailPim, None),
    ("com.apple.notes", MailPim, None),
    ("com.apple.reminders", MailPim, None),
    ("com.apple.remindd", MailPim, None),
    ("com.apple.voicememos", MailPim, None),
    ("com.apple.journal", MailPim, None),
    ("com.apple.freeform", MailPim, None),
    ("com.readdle.smartemail", MailPim, None),
    ("com.microsoft.outlook", MailPim, None),
    ("office", MailPim, None),
    ("it.bloop.airmail", MailPim, None),
    ("com.microsoft.word", Documents, Some(OFFICE_CONTAINER)),
    ("com.microsoft.excel", Documents, Some(OFFICE_CONTAINER)),
    ("com.microsoft.powerpoint", Documents, Some(OFFICE_CONTAINER)),
    ("com.apple.iwork", Documents, None),
    ("com.apple.textedit", Documents, None),
    ("com.apple.photos", Media, None),
    ("com.apple.finalcut", Media, None),
    ("com.apple.imovieapp", Media, None),
    ("com.apple.logic", Media, None),
    ("com.apple.garageband", Media, None),
    ("com.blackmagic-design", Media, None),
    ("com.captureone", Media, None),
    ("com.phaseone", Media, None),
    ("com.bohemiancoding.sketch3", Media, None),
    ("com.figma.desktop", Media, None),
    ("com.pixelmatorteam", Media, None),
    ("com.seriflabs", Media, None),
    ("com.utmapp", VirtualMachine, None),
    ("com.parallels", VirtualMachine, None),
    ("com.vmware.fusion", VirtualMachine, None),
    ("org.virtualbox", VirtualMachine, None),
    ("com.docker.docker", VirtualMachine, None),
    ("dev.orbstack", VirtualMachine, None),
    ("com.isaacmarovitz.whisky", VirtualMachine, None),
    ("com.codeweavers", VirtualMachine, None),
    ("codes.rambo.virtualbuddy", VirtualMachine, None),
    ("com.tinyapp.dbngin", Database, None),
    ("com.postgresapp", Database, None),
    ("md.obsidian", Notes, None),
    ("net.shinyfrog.bear", Notes, None),
    ("com.evernote", Notes, None),
    ("com.bloombuilt.dayone", Notes, None),
    ("dayoneapp", Notes, None),
    ("com.logseq", Notes, None),
    ("notion.id", Notes, None),
    ("com.culturedcode.things", Notes, None),
    ("com.omnigroup.omnifocus", Notes, None),
    ("com.ulyssesapp", Notes, None),
    ("com.soulmen.ulysses", Notes, None),
    ("com.agiletortoise.drafts", Notes, None),
    ("com.lukilabs.lukiapp", Notes, None),
    ("com.literatureandlatte.scrivener", Notes, None),
    ("com.automattic.simplenote", Notes, None),
    ("org.standardnotes", Notes, None),
    ("io.anytype", Notes, None),
    ("com.microsoft.onenote", Notes, None),
    ("com.valvesoftware.steam", GameSaves, None),
    ("org.openemu", GameSaves, None),
    ("com.feralinteractive", GameSaves, None),
    ("com.aspyr", GameSaves, None),
    ("net.pcsx2", GameSaves, None),
    ("net.rpcs3", GameSaves, None),
    ("org.dolphin-emu", GameSaves, None),
    ("org.ryujinx", GameSaves, None),
    ("com.libretro", GameSaves, None),
    ("com.gog", GameSaves, None),
    ("com.epicgames", GameSaves, None),
    ("unity.", GameSaves, None),
    ("com.getdropbox.dropbox", CloudSync, None),
    ("com.dropbox", CloudSync, None),
    ("com.google.drivefs", CloudSync, None),
    ("com.microsoft.onedrive", CloudSync, None),
    ("onedrive", CloudSync, None),
    ("com.box.desktop", CloudSync, None),
    ("com.pcloud", CloudSync, None),
    ("nz.mega", CloudSync, None),
    ("com.nextcloud", CloudSync, None),
    ("com.owncloud", CloudSync, None),
    ("com.synology", CloudSync, None),
    ("me.proton.drive", CloudSync, None),
    ("com.apple.clouddocs", CloudSync, None),
];

const DATA_BUNDLE_LOCATIONS: &[&str] = &["Application Support", "Containers", "Group Containers", "WebKit"];

const HOME_ROOTS: &[Root] = &[
    root(&[Lit(".ssh")], Credentials, NONE),
    root(&[Lit(".gnupg")], Credentials, NONE),
    root(&[Lit(".password-store")], Credentials, NONE),
    root(&[Lit(".aws")], Credentials, AWS),
    root(&[Lit(".azure")], Credentials, LOGS),
    root(&[Lit(".kube")], Credentials, KUBE),
    root(&[Lit(".config"), Lit("gcloud")], Credentials, LOGS),
    root(&[Lit(".netrc")], Credentials, NONE),
    root(&[Lit(".git-credentials")], Credentials, NONE),
    root(&[Lit(".pypirc")], Credentials, NONE),
    root(&[Lit(".docker")], Credentials, DOCKER_HOME),
    root(&[Lit(".npmrc")], Credentials, NONE),
    root(&[Lit(".sbt"), Lit(".credentials")], Credentials, NONE),
    root(&[Lit(".ivy2"), Lit(".credentials")], Credentials, NONE),
    root(&[Lit(".m2"), Lit("settings.xml")], Credentials, NONE),
    root(&[Lit(".m2"), Lit("settings-security.xml")], Credentials, NONE),
    root(&[Lit(".gradle"), Lit("gradle.properties")], Credentials, NONE),
    root(&[Lit(".electrum")], Wallet, NONE),
    root(&[Lit(".electrum-ltc")], Wallet, NONE),
    root(&[Lit(".bitcoin")], Wallet, NONE),
    root(&[Lit(".litecoin")], Wallet, NONE),
    root(&[Lit(".ethereum")], Wallet, NONE),
    root(&[Lit(".sparrow")], Wallet, NONE),
    root(&[Lit(".walletwasabi")], Wallet, NONE),
    root(&[Lit(".bitmonero")], Wallet, NONE),
    root(&[Lit(".zcash")], Wallet, NONE),
    root(&[Lit(".lnd")], Wallet, NONE),
    root(&[Lit(".lightning")], Wallet, NONE),
    root(&[Lit(".specter")], Wallet, NONE),
    root(&[Lit("Monero")], Wallet, NONE),
    root(&[Lit("Parallels")], VirtualMachine, NONE),
    root(&[Lit("Virtual Machines.localized")], VirtualMachine, NONE),
    root(&[Lit("Virtual Machines")], VirtualMachine, NONE),
    root(&[Lit("VirtualBox VMs")], VirtualMachine, VIRTUALBOX_VMS),
    root(&[Lit(".android"), Lit("avd")], VirtualMachine, NONE),
    root(&[Lit(".android"), Lit("adbkey")], Credentials, NONE),
    root(&[Lit(".android"), Lit("debug.keystore")], Credentials, NONE),
    root(&[Lit(".orbstack")], VirtualMachine, NONE),
    root(&[Lit(".tart")], VirtualMachine, NONE),
    root(&[Lit(".lima")], VirtualMachine, NONE),
    root(&[Lit(".colima")], VirtualMachine, NONE),
    root(&[Lit(".prometheus"), Lit("data")], Database, NONE),
    root(&[Lit(".logseq")], Notes, NONE),
    root(&[Lit("Music"), Lit("Music")], Media, NONE),
    root(&[Lit("Music"), Lit("iTunes")], Media, NONE),
    root(&[Lit("Music"), Lit("Audio Music Apps")], Media, NONE),
    root(&[Lit("Movies"), Lit("TV")], Media, NONE),
    root(&[Lit("Documents"), Lit("Adobe")], Media, NONE),
    root(&[Lit("Dropbox")], CloudSync, NONE),
    root(&[Prefix("Dropbox (")], CloudSync, NONE),
    root(&[Lit(".dropbox")], CloudSync, NONE),
    root(&[Lit("Google Drive")], CloudSync, NONE),
    root(&[Prefix("GoogleDrive")], CloudSync, NONE),
    root(&[Prefix("OneDrive")], CloudSync, NONE),
    root(&[Lit("Box")], CloudSync, NONE),
    root(&[Lit("Box Sync")], CloudSync, NONE),
    root(&[Lit("pCloud Drive")], CloudSync, NONE),
    root(&[Lit("MEGA")], CloudSync, NONE),
    root(&[Lit("MEGAsync")], CloudSync, NONE),
    root(&[Lit("Nextcloud")], CloudSync, NONE),
    root(&[Lit("ownCloud")], CloudSync, NONE),
    root(&[Lit("Seafile")], CloudSync, NONE),
    root(&[Lit("SynologyDrive")], CloudSync, NONE),
    root(&[Lit("Creative Cloud Files")], CloudSync, NONE),
    root(&[Prefix("iCloud Drive")], CloudSync, NONE),
    root(&[Prefix("Proton Drive")], CloudSync, NONE),
];

const ABS_ROOTS: &[Root] = &[
    root(&[Lit("opt"), Lit("homebrew"), Lit("var")], Database, HOMEBREW_VAR),
    root(&[Lit("usr"), Lit("local"), Lit("var")], Database, HOMEBREW_VAR),
    root(&[Lit("Users"), Lit("Shared"), Lit("Parallels")], VirtualMachine, NONE),
    root(&[Lit("Users"), Lit("Shared"), Lit("DBngin")], Database, NONE),
];

/// Single path components that are protected wherever they appear.
const ANYWHERE: &[(Seg, Category)] = &[
    (Lit(".git"), SourceCode),
    (Suffix(".git"), SourceCode),
    (Lit(".ipynb_checkpoints"), SourceCode),
    (Suffix(".ipynb"), SourceCode),
    (Suffix(".xcarchive"), SourceCode),
    (Lit(".obsidian"), Notes),
    (Lit("wallet.dat"), Wallet),
    (Prefix("UTC--"), Wallet),
    (Suffix(".keys"), Wallet),
    (Suffix(".kdbx"), Credentials),
    (Suffix(".kdb"), Credentials),
    (Suffix(".opvault"), Credentials),
    (Suffix(".agilekeychain"), Credentials),
    (Suffix(".keychain"), Credentials),
    (Suffix(".keychain-db"), Credentials),
    (Suffix(".mbox"), MailPim),
    (Suffix(".abbu"), MailPim),
    (Suffix(".icbu"), MailPim),
    (Suffix(".photoslibrary"), Media),
    (Suffix(".migratedphotolibrary"), Media),
    (Suffix(".aplibrary"), Media),
    (Suffix(".musiclibrary"), Media),
    (Suffix(".tvlibrary"), Media),
    (Suffix(".fcpbundle"), Media),
    (Suffix(".imovielibrary"), Media),
    (Suffix(".theater"), Media),
    (Suffix(".logicx"), Media),
    (Suffix(".band"), Media),
    (Suffix(".lrcat"), Media),
    (Suffix(".lrcat-data"), Media),
    (Suffix(".lrlibrary"), Media),
    (Suffix(".cocatalog"), Media),
    (Suffix(".cosessiondb"), Media),
    (Suffix(".dra"), Media),
    (Suffix(".sketch"), Media),
    (Suffix(".utm"), VirtualMachine),
    (Suffix(".pvm"), VirtualMachine),
    (Suffix(".vmwarevm"), VirtualMachine),
    (Suffix(".vbox"), VirtualMachine),
    (Suffix(".vbvm"), VirtualMachine),
    (Lit("saves"), GameSaves),
    (Lit("savegames"), GameSaves),
    (Lit("save games"), GameSaves),
    (Lit("saved games"), GameSaves),
    (Lit("savedata"), GameSaves),
    (Lit("save data"), GameSaves),
];

/// Children that make their folder protected (data directories of local
/// databases, repositories, vaults).
const CHILD_MARKERS: &[(&str, Category)] = &[
    ("PG_VERSION", Database),
    ("ibdata1", Database),
    ("mysql.ibd", Database),
    ("dump.rdb", Database),
    ("appendonly.aof", Database),
    ("appendonlydir", Database),
    ("WiredTiger", Database),
    ("mongod.lock", Database),
];

/// Files whose presence in an ancestor makes everything below protected.
const ANCESTOR_MARKERS: &[(&str, Category)] = &[
    (".obsidian", Notes),
    ("logseq/config.edn", Notes),
    ("PG_VERSION", Database),
    ("ibdata1", Database),
    ("WiredTiger", Database),
];

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn seg_matches(seg: &Seg, name: &str) -> bool {
    let n = name.as_bytes();
    match seg {
        Lit(l) => eq(l, name),
        Prefix(p) => n.len() >= p.len() && n[..p.len()].eq_ignore_ascii_case(p.as_bytes()),
        Suffix(s) => n.len() > s.len() && n[n.len() - s.len()..].eq_ignore_ascii_case(s.as_bytes()),
        Any => true,
    }
}

fn segs_match(segs: &[Seg], names: &[String]) -> bool {
    names.len() >= segs.len() && segs.iter().zip(names).all(|(s, n)| seg_matches(s, n))
}

fn segs_strict_prefix(segs: &[Seg], names: &[String]) -> bool {
    names.len() < segs.len() && segs.iter().zip(names).all(|(s, n)| seg_matches(s, n))
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

fn path_of(comps: &[String]) -> PathBuf {
    let mut p = PathBuf::from("/");
    p.extend(comps);
    p
}

struct Hit {
    len: usize,
    cat: Category,
    leaves: Leaves,
}

fn first_hit(roots: &[Root], names: &[String]) -> Option<Hit> {
    roots
        .iter()
        .find(|r| segs_match(r.segs, names))
        .map(|r| Hit { len: r.segs.len(), cat: r.cat, leaves: r.leaves })
}

/// Category of a `Library/<location>/<entry>` entry named after a bundle id.
pub fn bundle_category(entry: &str) -> Option<Category> {
    bundle_entry(entry).map(|(cat, _)| cat)
}

fn bundle_entry(entry: &str) -> Option<(Category, Option<Leaves>)> {
    let id = browser_guard::entry_bundle_id(entry).to_ascii_lowercase();
    DATA_BUNDLES
        .iter()
        .find(|(p, _, _)| id.starts_with(p))
        .map(|(_, cat, leaves)| (*cat, *leaves))
}

fn library_hit(rest: &[String]) -> Option<Hit> {
    if let Some(hit) = first_hit(LIBRARY_ROOTS, rest) {
        return Some(hit);
    }
    if rest.len() < 2 {
        return None;
    }
    if eq(&rest[0], "Application Support") {
        if let Some((_, cat, leaves)) = APP_SUPPORT.iter().find(|(s, _, _)| seg_matches(s, &rest[1])) {
            return Some(Hit { len: 2, cat: *cat, leaves });
        }
    }
    let location = DATA_BUNDLE_LOCATIONS.iter().find(|l| eq(l, &rest[0]))?;
    let (cat, container_leaves) = bundle_entry(&rest[1])?;
    let leaves = match *location {
        "Containers" => container_leaves.unwrap_or(CONTAINER),
        "Group Containers" => GROUP,
        "Application Support" => ELECTRON,
        _ => NONE,
    };
    Some(Hit { len: 2, cat, leaves })
}

fn library_contains(rest: &[String]) -> bool {
    (rest.len() == 1 && DATA_BUNDLE_LOCATIONS.iter().any(|l| eq(l, &rest[0])))
        || LIBRARY_ROOTS.iter().any(|r| segs_strict_prefix(r.segs, rest))
}

/// Category of a single path component protected wherever it appears.
pub fn anywhere_category(name: &str) -> Option<Category> {
    ANYWHERE.iter().find(|(s, _)| seg_matches(s, name)).map(|(_, c)| *c)
}

fn leaf_matches(leaves: Leaves, rel: &[String]) -> bool {
    leaves.iter().any(|leaf| segs_match(leaf, rel))
}

fn real_home_components() -> Vec<Vec<String>> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        if let Some(c) = components(&home) {
            out.push(c);
        }
        if let Some(c) = fs::canonicalize(&home).ok().and_then(|h| components(&h)) {
            out.push(c);
        }
    }
    out
}

/// Indices into `comps` at which a home folder ends, from the path alone.
fn static_homes(comps: &[String]) -> Vec<usize> {
    let mut homes = Vec::new();
    if comps.len() >= 2 && eq(&comps[0], "Users") && !eq(&comps[1], "Shared") {
        homes.push(2);
    }
    for home in real_home_components() {
        if comps.len() >= home.len() && home.iter().zip(comps).all(|(a, b)| eq(a, b)) {
            homes.push(home.len());
        }
    }
    homes
}

fn looks_like_home(dir: &Path) -> bool {
    ["Library/Application Support", "Library/Preferences", "Library/Containers"]
        .iter()
        .any(|c| dir.join(c).is_dir())
}

/// Ancestors of the path (and the path itself) that look like a home.
fn fs_homes(comps: &[String]) -> Vec<usize> {
    (1..=comps.len())
        .rev()
        .take(16)
        .filter(|&k| looks_like_home(&path_of(&comps[..k])))
        .collect()
}

fn protected(category: Category, detail: &'static str, contains: bool) -> Verdict {
    Verdict::Protected { category, detail, contains }
}

fn classify(comps: &[String], homes: &[usize]) -> Verdict {
    // The outermost root decides which cache leaves are allowed.
    for i in 0..comps.len() {
        let mut hit = if i == 0 { first_hit(ABS_ROOTS, comps) } else { None };
        if hit.is_none() && homes.contains(&i) {
            hit = first_hit(HOME_ROOTS, &comps[i..]);
        }
        if hit.is_none() && i > 0 && eq(&comps[i - 1], "Library") {
            hit = library_hit(&comps[i..]);
        }
        if hit.is_none() {
            hit = anywhere_category(&comps[i]).map(|cat| Hit { len: 1, cat, leaves: NONE });
        }
        let Some(hit) = hit else { continue };
        let rel = &comps[i + hit.len..];
        if rel.is_empty() {
            return protected(hit.cat, "protected root", false);
        }
        if leaf_matches(hit.leaves, rel) && !rel.iter().any(|c| anywhere_category(c).is_some()) {
            return Verdict::CacheLeaf;
        }
        return protected(hit.cat, "inside protected data", false);
    }

    let contains = |cat| protected(cat, "contains protected data", true);
    if comps.is_empty() || (eq(&comps[0], "Users") && comps.len() <= 2) {
        return contains(UserData);
    }
    if let Some(r) = ABS_ROOTS.iter().find(|r| segs_strict_prefix(r.segs, comps)) {
        return contains(r.cat);
    }
    if real_home_components()
        .iter()
        .any(|h| comps.len() < h.len() && comps.iter().zip(h).all(|(a, b)| eq(a, b)))
    {
        return contains(UserData);
    }
    for &h in homes {
        if h == comps.len() {
            return contains(UserData);
        }
        if let Some(r) = HOME_ROOTS.iter().find(|r| segs_strict_prefix(r.segs, &comps[h..])) {
            return contains(r.cat);
        }
        if h + 1 == comps.len() && eq(&comps[h], "Library") {
            return contains(UserData);
        }
    }
    for i in 1..comps.len() {
        if eq(&comps[i - 1], "Library") && library_contains(&comps[i..]) {
            let cat = LIBRARY_ROOTS
                .iter()
                .find(|r| segs_strict_prefix(r.segs, &comps[i..]))
                .map_or(UserData, |r| r.cat);
            return contains(cat);
        }
    }
    Verdict::NotProtected
}

/// Path-only classification: no filesystem access, so rule and probe
/// paths can be checked in tests.
pub fn static_verdict(path: &Path) -> Verdict {
    match components(path) {
        Some(comps) => classify(&comps, &static_homes(&comps)),
        None => protected(UserData, "unnormalised path", false),
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn child_category(parent: &Path, name: &str) -> Option<Category> {
    if let Some(cat) = anywhere_category(name) {
        return Some(cat);
    }
    if let Some((_, cat)) = CHILD_MARKERS.iter().find(|(m, _)| eq(m, name)) {
        return Some(*cat);
    }
    if eq(name, "logseq") && parent.join(name).join("config.edn").is_file() {
        return Some(Notes);
    }
    if eq(name, "Library") && looks_like_home(parent) {
        return Some(UserData);
    }
    None
}

/// Category of protected data directly inside `dir`, if any.
fn child_marker(dir: &Path) -> Option<Category> {
    const MAX_ENTRIES: usize = 20_000;
    if !is_real_dir(dir) {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .take(MAX_ENTRIES)
        .find_map(|e| child_category(dir, &e.file_name().to_string_lossy()))
}

fn ancestor_marker(path: &Path) -> Option<Category> {
    path.ancestors().skip(1).take(8).find_map(|anc| {
        ANCESTOR_MARKERS
            .iter()
            .find(|(m, _)| anc.join(m).exists())
            .map(|(_, cat)| *cat)
    })
}

/// Home folders that macOS mirrors into iCloud Drive ("Desktop & Documents
/// Folders"). Deleting inside them deletes the files from iCloud as well.
fn icloud_synced_folders() -> Vec<PathBuf> {
    #[cfg(test)]
    if let Some(dirs) = tests_support::synced_override() {
        return dirs;
    }
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, dirs)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return dirs.clone();
        }
    }
    let dirs = dirs::home_dir()
        .map(|home| synced_folders_from(&home, finder_flag))
        .unwrap_or_default();
    *cache = Some((Instant::now(), dirs.clone()));
    dirs
}

fn finder_flag(key: &str) -> Option<bool> {
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "com.apple.finder", key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

pub(crate) fn synced_folders_from(home: &Path, flag: impl Fn(&str) -> Option<bool>) -> Vec<PathBuf> {
    let cloud_docs = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    [("FXICloudDriveDesktop", "Desktop"), ("FXICloudDriveDocuments", "Documents")]
        .into_iter()
        .filter(|(key, name)| match flag(key) {
            Some(on) => on,
            // Unknown setting: assume synced if iCloud Drive holds the mirror folder.
            None => cloud_docs.join(name).is_dir(),
        })
        .map(|(_, name)| home.join(name))
        .collect()
}

fn in_synced_folder(path: &Path) -> bool {
    icloud_synced_folders().iter().any(|dir| path.starts_with(dir))
}

/// Filesystem-aware verdict: `static_verdict` plus homes found on disk,
/// marker children (database data dirs, repositories, vaults) and marker
/// ancestors.
pub fn verdict(path: &Path) -> Verdict {
    let Some(comps) = components(path) else {
        return protected(UserData, "unnormalised path", false);
    };
    let mut homes = static_homes(&comps);
    homes.extend(fs_homes(&comps));
    let v = classify(&comps, &homes);
    if v != Verdict::NotProtected {
        return v;
    }
    if in_synced_folder(&path_of(&comps)) {
        return protected(CloudSync, "synced to iCloud Drive", false);
    }
    let own_name = comps.last().map(String::as_str).unwrap_or("");
    if let Some((_, cat)) = CHILD_MARKERS.iter().find(|(m, _)| eq(m, own_name)) {
        return protected(*cat, "protected root", false);
    }
    if let Some(cat) = child_marker(path) {
        return protected(cat, "contains protected data", true);
    }
    if let Some(cat) = ancestor_marker(path) {
        return protected(cat, "inside protected data", false);
    }
    Verdict::NotProtected
}

fn check_one(path: &Path, ctx: DeleteContext) -> Result<(), Refusal> {
    match verdict(path) {
        Verdict::NotProtected | Verdict::CacheLeaf => Ok(()),
        Verdict::Protected { category, detail, contains } => {
            let confirmed = matches!(ctx, DeleteContext::Confirmed) && !matches!(category, CloudSync | UserData);
            if confirmed {
                return Ok(());
            }
            Err(Refusal { path: path.to_string_lossy().into_owned(), category, detail, contains })
        }
    }
}

/// Decides whether `path` may be deleted. Checks browser data first, then
/// every other protected category, for the literal path and, if it
/// resolves elsewhere, the resolved one too.
pub fn check(path: &Path, ctx: DeleteContext) -> Result<(), Refusal> {
    let browser = match ctx {
        DeleteContext::Uninstall { bundle_id } => {
            browser_guard::check(path, browser_guard::DeleteContext::Uninstall { bundle_id })
        }
        _ => browser_guard::check_general(path),
    };
    browser.map_err(Refusal::from_browser)?;
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

/// Splits `path` into what may be deleted and what must be kept: a folder
/// refused only because it holds protected data is replaced by its
/// children, down to `depth` levels.
pub fn split(path: &Path, ctx: DeleteContext, depth: usize) -> (Vec<PathBuf>, Vec<Refusal>) {
    match check(path, ctx) {
        Ok(()) => (vec![path.to_path_buf()], Vec::new()),
        Err(r) if r.contains && depth > 0 && is_real_dir(path) => {
            let mut ok = Vec::new();
            let mut kept = Vec::new();
            if let Ok(entries) = fs::read_dir(path) {
                for e in entries.flatten() {
                    let (a, b) = split(&e.path(), ctx, depth - 1);
                    ok.extend(a);
                    kept.extend(b);
                }
            }
            (ok, kept)
        }
        Err(r) => (Vec::new(), vec![r]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeScan {
    Clean,
    Found(Category),
    TooBig,
}

/// Looks for protected data up to `max_depth` levels below `dir`, visiting
/// at most `budget` folders. Browser profiles are not looked for: Electron
/// apps carry the same markers (see `browser_guard::contains_profile_markers`).
pub fn scan_tree(dir: &Path, max_depth: usize, budget: usize) -> TreeScan {
    if !is_real_dir(dir) {
        return TreeScan::Clean;
    }
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((d, depth)) = stack.pop() {
        visited += 1;
        if visited > budget {
            return TreeScan::TooBig;
        }
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(cat) = child_category(&d, &name) {
                return TreeScan::Found(cat);
            }
            if depth < max_depth && e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    TreeScan::Clean
}

#[cfg(test)]
pub mod fixtures {
    //! A fake home holding realistic irreplaceable data of every protected
    //! category, for tests that run real delete code against it.

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::commands::test_support::{set_age_days, set_quarantine};

    pub struct FakeData {
        pub home: PathBuf,
        /// Every protected file, with its original contents.
        pub protected: BTreeMap<PathBuf, Vec<u8>>,
        /// Regenerable cache files inside protected roots.
        pub cache_leaves: Vec<PathBuf>,
        /// Protected roots and folders that contain them.
        pub protected_dirs: Vec<PathBuf>,
    }

    impl FakeData {
        /// Panics naming every protected file that was removed or changed.
        pub fn assert_intact(&self) {
            self.assert_intact_except(&[]);
        }

        /// Like `assert_intact`, ignoring files under the given home-relative
        /// paths (data the user explicitly approved deleting).
        pub fn assert_intact_except(&self, approved: &[&str]) {
            let damaged: Vec<String> = self
                .protected
                .iter()
                .filter(|(p, _)| !approved.iter().any(|a| p.starts_with(self.home.join(a))))
                .filter(|(p, bytes)| fs::read(p).ok().as_deref() != Some(bytes.as_slice()))
                .map(|(p, _)| p.strip_prefix(&self.home).unwrap_or(p).display().to_string())
                .collect();
            assert!(damaged.is_empty(), "protected user data damaged: {damaged:#?}");
        }

        pub fn path(&self, rel: &str) -> PathBuf {
            self.home.join(rel)
        }
    }

    const LIB: &str = "Library";
    const AS: &str = "Library/Application Support";
    const GC: &str = "Library/Group Containers";
    const CT: &str = "Library/Containers";

    /// Irreplaceable files, relative to the home.
    pub const PROTECTED: &[&str] = &[
        // Passwords and keys
        ".ssh/id_ed25519",
        ".ssh/known_hosts",
        ".gnupg/private-keys-v1.d/0123ABCD.key",
        ".aws/credentials",
        "Library/Keychains/login.keychain-db",
        "Library/Group Containers/2BUA8C4S2C.com.1password/Library/Application Support/1Password/Data/1password.sqlite",
        "Library/Application Support/Bitwarden/data.json",
        "Library/Application Support/Authy Desktop/Local Storage/leveldb/000003.log",
        "Documents/Passwords.kdbx",
        // Crypto wallets
        ".electrum/wallets/default_wallet",
        ".ethereum/keystore/UTC--2021-01-01T00-00-00.000Z--0123456789abcdef",
        ".sparrow/wallets/hot.mv.db",
        ".walletwasabi/client/Wallets/Main.json",
        "Library/Application Support/Bitcoin/wallets/main/wallet.dat",
        "Library/Application Support/Ledger Live/app.json",
        "Library/Application Support/Exodus/exodus.wallet/seed.seco",
        "Library/Application Support/@trezor/suite-desktop/Local Storage/leveldb/000003.log",
        "Monero/wallets/main/main.keys",
        "Documents/Crypto/wallet.dat",
        // Message history
        "Library/Messages/chat.db",
        "Library/Messages/Attachments/ab/01/IMG_0001.HEIC",
        "Library/Application Support/Signal/sql/db.sqlite",
        "Library/Application Support/Signal/attachments.noindex/ab/abcdef",
        "Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram/stable/account-123/postbox/db/db_sqlite",
        "Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram/stable/account-123/postbox/media/12345",
        "Library/Application Support/Telegram Desktop/tdata/key_datas",
        "Library/Group Containers/group.net.whatsapp.WhatsApp.shared/ChatStorage.sqlite",
        "Library/Group Containers/group.net.whatsapp.WhatsApp.shared/Message/Media/a/b/photo.jpg",
        "Library/Containers/net.whatsapp.WhatsApp/Data/Library/Application Support/state.db",
        "Library/Containers/net.whatsapp.WhatsApp/Data/tmp/outgoing.jpg",
        "Library/Application Support/Slack/IndexedDB/https_app.slack.com_0.indexeddb.leveldb/000003.log",
        "Library/Application Support/Slack/Service Worker/CacheStorage/abc/index.txt",
        "Library/Application Support/discord/Local Storage/leveldb/000003.log",
        "Library/Application Support/Microsoft/Teams/IndexedDB/x/000003.log",
        "Library/Containers/com.tencent.xinWeChat/Data/Library/Application Support/com.tencent.xinWeChat/2.0b4.0.9/msg.db",
        "Library/Containers/jp.naver.line.mac/Data/Documents/talk.sqlite",
        "Library/Application Support/ViberPC/123/viber.db",
        // Mail, contacts, calendars, notes
        "Library/Mail/V10/MailData/Envelope Index",
        "Library/Mail/V10/ABC/INBOX.mbox/Messages/1.emlx",
        "Library/Calendars/Calendar Cache",
        "Library/Application Support/AddressBook/Sources/ABC/AddressBook-v22.abcddb",
        "Library/Group Containers/group.com.apple.notes/NoteStore.sqlite",
        "Library/Group Containers/group.com.apple.reminders/Container_v1/Stores/Data-local.sqlite",
        "Library/Autosave Information/Unsaved TextEdit Document.rtf",
        "Library/Containers/com.microsoft.Word/Data/Library/Preferences/AutoRecovery/Document.asd",
        // Media libraries
        "Pictures/Photos Library.photoslibrary/database/Photos.sqlite",
        "Pictures/Photos Library.photoslibrary/originals/0/IMG_0001.heic",
        "Music/Music/Music Library.musiclibrary/Library.musicdb",
        "Music/Music/Media.localized/Music/Artist/Album/01 Song.m4a",
        "Movies/My Film.fcpbundle/Project/CurrentVersion.fcpevent",
        "Movies/My Film.fcpbundle/Project/Render Files/High Quality Media/render.mov",
        "Music/GarageBand/Song.band/projectData",
        "Music/Logic/Track.logicx/Alternatives/000/ProjectData",
        "Library/Application Support/Blackmagic Design/DaVinci Resolve/Resolve Disk Database/Resolve Projects/Users/guest/Projects/Film/Project.db",
        "Pictures/Lightroom/Lightroom Catalog.lrcat",
        "Pictures/Capture One Catalog.cocatalog/Capture One Catalog.cocatalogdb",
        "Documents/Adobe/Premiere Pro/24.0/Adobe Premiere Pro Auto-Save/Edit--1.prproj",
        "Documents/Design.sketch",
        // Device backups and virtual machines
        "Library/Application Support/MobileSync/Backup/00008030-001A/Manifest.db",
        "Library/Application Support/MobileSync/Backup/00008030-001A/ab/abcdef0123",
        "Library/Containers/com.utmapp.UTM/Data/Documents/Linux.utm/config.plist",
        "Library/Containers/com.utmapp.UTM/Data/Documents/Linux.utm/Data/disk.qcow2",
        "Parallels/Windows 11.pvm/config.pvs",
        "Virtual Machines.localized/Ubuntu.vmwarevm/Ubuntu.vmx",
        "VirtualBox VMs/Debian/Debian.vdi",
        ".android/avd/Pixel_7.avd/userdata-qemu.img",
        "Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img",
        "Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw",
        // Databases and developer state
        "Library/Application Support/Postgres/var-16/PG_VERSION",
        "Library/Application Support/Postgres/var-16/base/1/1259",
        "Developer/db/pgdata/PG_VERSION",
        "Developer/db/pgdata/base/1/1259",
        "Developer/db/mysql/ibdata1",
        "Developer/db/mysql/app/users.ibd",
        "Developer/db/redis/dump.rdb",
        "Projects/notebooks/analysis.ipynb",
        "Projects/notebooks/.ipynb_checkpoints/analysis-checkpoint.ipynb",
        "Projects/app/.git/HEAD",
        "Projects/app/.git/objects/ab/cdef0123",
        "Library/Developer/Xcode/Archives/2026-01-01/App 1-1-26.xcarchive/dSYMs/App.app.dSYM/Contents/Info.plist",
        "Library/Application Support/JetBrains/IntelliJIdea2024.1/scratches/scratch.kt",
        // Notes
        "Documents/Vault/.obsidian/workspace.json",
        "Documents/Vault/Daily/2026-09-29.md",
        "Documents/logseq-graph/logseq/config.edn",
        "Documents/logseq-graph/pages/Home.md",
        "Library/Group Containers/9K33E3U3T4.net.shinyfrog.bear/Application Data/database.sqlite",
        "Library/Group Containers/5U8NS4GX82.dayoneapp2/Data/Documents/DayOne.sqlite",
        "Library/Application Support/Notion/notion.db",
        "Library/Containers/com.evernote.Evernote/Data/Library/Application Support/com.evernote.Evernote/accounts/notes.db",
        // Game saves
        "Library/Application Support/Steam/userdata/12345/730/remote/cfg.vdf",
        "Library/Application Support/minecraft/saves/World/level.dat",
        "Library/Application Support/OpenEmu/Save States/SNES/slot1.oesavestate",
        "Library/Application Support/unity.Studio.Game/save.json",
        "Library/Application Support/com.Studio.Game/Saves/slot1.sav",
        // Cloud-synced folders
        "Library/Mobile Documents/com~apple~CloudDocs/Taxes/2025.pdf",
        "Library/Mobile Documents/com~apple~CloudDocs/Downloads/Installer.dmg",
        "Library/Mobile Documents/com~apple~CloudDocs/Projects/site/package.json",
        "Library/Mobile Documents/com~apple~CloudDocs/Projects/site/node_modules/left-pad/index.js",
        "Library/CloudStorage/Dropbox/Work/plan.docx",
        "Library/CloudStorage/GoogleDrive-me@example.com/My Drive/budget.xlsx",
        "Library/Application Support/Google/DriveFS/1234/content_cache/pending.bin",
        "Dropbox/Photos/beach.jpg",
    ];

    /// The user's own files outside protected roots that modules must
    /// still never touch: source projects, a hand-written `build/` folder
    /// with no build-tool output, disk images the user made, and the only
    /// copy of an app.
    pub const USER_FILES: &[&str] = &[
        "AndroidStudioProjects/MyApp/app/src/main/java/Main.kt",
        "Projects/app/src/main.rs",
        "DevEcoStudioProjects/App/entry/src/main/ets/Index.ets",
        "Projects/site/package.json",
        "Projects/site/Makefile",
        "Projects/site/build/notes.md",
        "Projects/site/build/deploy.sh",
        "Projects/site/dist/README.txt",
        "Projects/rusty/Cargo.toml",
        "Projects/rusty/target/design.md",
        "Documents/Releases/MyApp-1.0.dmg",
        "Documents/Releases/MyApp-1.0.zip",
        "Desktop/Scans.iso",
    ];

    /// Downloaded (quarantined) files the user still wants: an encrypted
    /// disk image and an app whose only copy lives in Downloads.
    pub const QUARANTINED_USER_FILES: &[&str] = &[
        "Downloads/Vault.dmg",
        "Downloads/OnlyCopy.app/Contents/Info.plist",
    ];

    /// Regenerable cache files inside protected roots.
    pub const CACHE_LEAVES: &[&str] = &[
        "Library/Application Support/Signal/Cache/Cache_Data/f_000001",
        "Library/Application Support/Slack/Cache/Cache_Data/f_000001",
        "Library/Application Support/Slack/Code Cache/js/index",
        "Library/Application Support/Slack/GPUCache/data_0",
        "Library/Application Support/discord/Cache/Cache_Data/f_000001",
        "Library/Application Support/discord/Code Cache/js/index",
        "Library/Application Support/Microsoft/Teams/Cache/f_000001",
        "Library/Application Support/Microsoft/Teams/logs/teams.log",
        "Library/Messages/StickerCache/sticker.png",
        "Library/Messages/Caches/Previews/Attachments/preview.png",
        "Library/Containers/com.tencent.xinWeChat/Data/Library/Caches/img.bin",
        "Library/Containers/com.utmapp.UTM/Data/Library/Caches/thumb.bin",
        "Library/Containers/com.microsoft.Word/Data/tmp/word.tmp",
        "Library/Containers/com.apple.mail/Data/Library/Mail Downloads/ABC/attachment.pdf",
        "Library/Application Support/Blackmagic Design/DaVinci Resolve/CacheClip/clip.dvcc",
        "Library/Application Support/Blackmagic Design/DaVinci Resolve/Cache/frame.bin",
        "Library/Application Support/Adobe/Common/Media Cache Files/audio.cfa",
        "Library/Application Support/Notion/Cache/Cache_Data/f_000001",
        "Library/Application Support/Steam/appcache/appinfo.vdf",
        "Library/Application Support/Steam/htmlcache/Cache/f_000001",
        "Library/Application Support/Steam/steamapps/shadercache/730/shader.bin",
        "Library/Application Support/minecraft/logs/latest.log",
        "Library/Developer/Xcode/UserData/IB Support/Simulator/x.bin",
        ".aws/cli/cache/token.json",
        ".kube/cache/discovery/x.json",
        ".docker/buildx/cache/layer.bin",
    ];

    /// Roots and folders holding roots, relative to the home.
    const PROTECTED_DIRS: &[&str] = &[
        "",
        LIB,
        AS,
        GC,
        CT,
        ".ssh",
        ".electrum",
        ".ethereum",
        ".ethereum/keystore",
        "Library/Keychains",
        "Library/Group Containers/2BUA8C4S2C.com.1password",
        "Library/Application Support/Bitwarden",
        "Library/Application Support/Bitcoin",
        "Library/Application Support/Bitcoin/wallets/main",
        "Library/Application Support/Ledger Live",
        "Library/Application Support/Exodus",
        "Monero",
        "Documents",
        "Documents/Crypto",
        "Library/Messages",
        "Library/Messages/Attachments",
        "Library/Application Support/Signal",
        "Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram",
        "Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram/stable/account-123/postbox/media",
        "Library/Application Support/Telegram Desktop",
        "Library/Group Containers/group.net.whatsapp.WhatsApp.shared",
        "Library/Containers/net.whatsapp.WhatsApp",
        "Library/Containers/net.whatsapp.WhatsApp/Data/tmp",
        "Library/Application Support/Slack",
        "Library/Application Support/Slack/Service Worker",
        "Library/Application Support/discord",
        "Library/Application Support/Microsoft",
        "Library/Application Support/Microsoft/Teams",
        "Library/Containers/com.tencent.xinWeChat",
        "Library/Mail",
        "Library/Calendars",
        "Library/Application Support/AddressBook",
        "Library/Group Containers/group.com.apple.notes",
        "Library/Autosave Information",
        "Library/Containers/com.microsoft.Word",
        "Pictures",
        "Pictures/Photos Library.photoslibrary",
        "Music",
        "Music/Music",
        "Movies",
        "Movies/My Film.fcpbundle",
        "Library/Application Support/Blackmagic Design",
        "Library/Application Support/Blackmagic Design/DaVinci Resolve",
        "Library/Application Support/Blackmagic Design/DaVinci Resolve/Resolve Disk Database",
        "Documents/Adobe",
        "Library/Application Support/MobileSync",
        "Library/Application Support/MobileSync/Backup",
        "Library/Containers/com.utmapp.UTM",
        "Library/Containers/com.utmapp.UTM/Data/Documents/Linux.utm",
        "Parallels",
        "VirtualBox VMs",
        ".android",
        ".android/avd",
        "Library/Group Containers/HUAQ24HBR6.dev.orbstack",
        "Library/Containers/com.docker.docker",
        "Library/Application Support/Postgres",
        "Developer/db/pgdata",
        "Developer/db/pgdata/base",
        "Developer/db/mysql",
        "Developer/db/redis",
        "Projects/notebooks/.ipynb_checkpoints",
        "Projects/app",
        "Projects/app/.git",
        "Library/Developer",
        "Library/Developer/Xcode",
        "Library/Developer/Xcode/Archives",
        "Library/Application Support/JetBrains",
        "Library/Application Support/JetBrains/IntelliJIdea2024.1",
        "Documents/Vault",
        "Documents/Vault/Daily",
        "Documents/logseq-graph",
        "Documents/logseq-graph/pages",
        "Library/Group Containers/9K33E3U3T4.net.shinyfrog.bear",
        "Library/Application Support/Notion",
        "Library/Containers/com.evernote.Evernote",
        "Library/Application Support/Steam",
        "Library/Application Support/Steam/userdata",
        "Library/Application Support/minecraft",
        "Library/Application Support/minecraft/saves",
        "Library/Application Support/OpenEmu",
        "Library/Application Support/unity.Studio.Game",
        "Library/Application Support/com.Studio.Game",
        "Library/Mobile Documents",
        "Library/Mobile Documents/com~apple~CloudDocs/Downloads",
        "Library/Mobile Documents/com~apple~CloudDocs/Projects/site/node_modules",
        "Library/CloudStorage",
        "Library/CloudStorage/Dropbox",
        "Library/Application Support/Google/DriveFS",
        "Dropbox",
    ];

    /// Top-level Library entries aged past every age gate.
    const AGED_PARENTS: &[&str] = &[AS, GC, CT, "Library/Caches", "Library/Logs"];

    fn put(home: &Path, rel: &str, tag: usize) -> (PathBuf, Vec<u8>) {
        let p = home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut bytes = format!("{rel}#{tag}\n").into_bytes();
        bytes.resize(4096, b'd');
        fs::write(&p, &bytes).unwrap();
        (p, bytes)
    }

    pub fn build(home: &Path) -> FakeData {
        let mut protected = BTreeMap::new();
        for (i, rel) in PROTECTED.iter().chain(USER_FILES).enumerate() {
            let (p, b) = put(home, rel, i + 1);
            protected.insert(p, b);
        }
        let vault = home.join("Downloads/Vault.dmg");
        fs::create_dir_all(vault.parent().unwrap()).unwrap();
        let mut bytes = b"encrcdsa".to_vec();
        bytes.resize(8192, 0);
        fs::write(&vault, &bytes).unwrap();
        set_quarantine(&vault);
        protected.insert(vault, bytes);
        let plist = home.join("Downloads/OnlyCopy.app/Contents/Info.plist");
        fs::create_dir_all(plist.parent().unwrap()).unwrap();
        let bytes = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.example.onlycopy</string></dict></plist>
"#
        .to_vec();
        fs::write(&plist, &bytes).unwrap();
        set_quarantine(&home.join("Downloads/OnlyCopy.app"));
        protected.insert(plist, bytes);
        let cache_leaves = CACHE_LEAVES.iter().map(|rel| put(home, rel, 0).0).collect();
        fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        fs::create_dir_all(home.join("Library/Caches")).unwrap();
        fs::create_dir_all(home.join("Library/Logs")).unwrap();
        for parent in AGED_PARENTS {
            for e in fs::read_dir(home.join(parent)).unwrap().flatten() {
                set_age_days(&e.path(), 400);
            }
        }
        let protected_dirs = PROTECTED_DIRS.iter().map(|rel| home.join(rel)).collect();
        FakeData { home: home.to_path_buf(), protected, cache_leaves, protected_dirs }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static SYNCED: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
    }

    pub fn synced_override() -> Option<Vec<PathBuf>> {
        SYNCED.with(|s| s.borrow().clone()).or_else(|| Some(Vec::new()))
    }

    pub fn set_synced(dirs: Vec<PathBuf>) {
        SYNCED.with(|s| *s.borrow_mut() = Some(dirs));
    }

    pub fn clear_synced() {
        SYNCED.with(|s| *s.borrow_mut() = None);
    }
}

#[cfg(test)]
mod icloud_tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target/kyra-test-tmp")
            .join(format!("icloud-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Desktop/project/node_modules")).unwrap();
        fs::create_dir_all(dir.join("Documents")).unwrap();
        fs::create_dir_all(dir.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
        dir
    }

    #[test]
    fn finder_setting_decides_which_folders_are_synced() {
        let home = temp_home("flags");
        let on = |k: &str| Some(k == "FXICloudDriveDesktop");
        assert_eq!(synced_folders_from(&home, on), vec![home.join("Desktop")]);
        assert!(synced_folders_from(&home, |_| Some(false)).is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn unknown_setting_falls_back_to_icloud_mirror_folders() {
        let home = temp_home("fallback");
        assert!(synced_folders_from(&home, |_| None).is_empty());
        fs::create_dir_all(home.join("Library/Mobile Documents/com~apple~CloudDocs/Documents")).unwrap();
        assert_eq!(synced_folders_from(&home, |_| None), vec![home.join("Documents")]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn nothing_inside_synced_desktop_can_be_deleted() {
        let home = temp_home("guard");
        tests_support::set_synced(vec![home.join("Desktop")]);
        let nm = home.join("Desktop/project/node_modules");
        let err = check(&nm, DeleteContext::General).unwrap_err();
        assert_eq!(err.category, CloudSync);
        let confirmed = check(&nm, DeleteContext::Confirmed).unwrap_err();
        assert_eq!(confirmed.category, CloudSync);
        tests_support::set_synced(Vec::new());
        assert!(check(&nm, DeleteContext::General).is_ok());
        tests_support::clear_synced();
        let _ = fs::remove_dir_all(&home);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::{canon, s, workspace_tempdir, write_file};

    const H: &str = "/Users/tester";

    fn v(rel: &str) -> Verdict {
        static_verdict(&Path::new(H).join(rel))
    }

    fn is_protected(rel: &str) -> bool {
        matches!(v(rel), Verdict::Protected { .. })
    }

    fn category(rel: &str) -> Option<Category> {
        match v(rel) {
            Verdict::Protected { category, .. } => Some(category),
            _ => None,
        }
    }

    #[test]
    fn every_category_has_protected_roots() {
        for (rel, cat) in [
            (".ssh/id_ed25519", Credentials),
            ("Library/Keychains/login.keychain-db", Credentials),
            ("Library/Group Containers/2BUA8C4S2C.com.1password", Credentials),
            ("Library/Application Support/Bitwarden", Credentials),
            ("Library/Containers/com.bitwarden.desktop", Credentials),
            ("Documents/db.kdbx", Credentials),
            (".electrum", Wallet),
            ("Library/Application Support/Bitcoin/wallets", Wallet),
            (".ethereum/keystore", Wallet),
            ("Library/Application Support/Ledger Live", Wallet),
            ("Library/Application Support/Exodus", Wallet),
            (".sparrow", Wallet),
            (".walletwasabi", Wallet),
            ("Library/Application Support/@trezor", Wallet),
            ("Library/Application Support/Trezor Suite", Wallet),
            ("Library/Application Support/atomic", Wallet),
            ("Library/Application Support/Coinomi", Wallet),
            ("Monero/wallets", Wallet),
            ("Library/Application Support/Frame", Wallet),
            ("Downloads/backup/wallet.dat", Wallet),
            ("Library/Messages/chat.db", Messages),
            ("Library/Messages/Attachments", Messages),
            ("Library/Application Support/Signal", Messages),
            ("Library/Application Support/Telegram Desktop/tdata", Messages),
            ("Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram", Messages),
            ("Library/Group Containers/group.net.whatsapp.WhatsApp.shared", Messages),
            ("Library/Containers/net.whatsapp.WhatsApp", Messages),
            ("Library/Application Support/Slack/IndexedDB", Messages),
            ("Library/Application Support/discord/Local Storage", Messages),
            ("Library/Application Support/Microsoft/Teams/IndexedDB", Messages),
            ("Library/Containers/com.microsoft.teams2", Messages),
            ("Library/Containers/com.tencent.xinWeChat", Messages),
            ("Library/Containers/jp.naver.line.mac", Messages),
            ("Library/Application Support/ViberPC", Messages),
            ("Library/Mail/V10", MailPim),
            ("Library/Calendars", MailPim),
            ("Library/Application Support/AddressBook", MailPim),
            ("Library/Group Containers/group.com.apple.notes", MailPim),
            ("Library/Group Containers/group.com.apple.reminders", MailPim),
            ("Library/Autosave Information", Documents),
            ("Pictures/Photos Library.photoslibrary", Media),
            ("Music/Music", Media),
            ("Movies/TV", Media),
            ("Movies/Film.fcpbundle/Render Files", Media),
            ("Movies/Trip.imovielibrary", Media),
            ("Music/Logic/Song.logicx", Media),
            ("Music/GarageBand/Song.band", Media),
            ("Library/Application Support/Blackmagic Design/DaVinci Resolve/Resolve Disk Database", Media),
            ("Pictures/Lightroom/Lightroom Catalog.lrcat", Media),
            ("Pictures/Catalog.cocatalog", Media),
            ("Documents/Adobe/Premiere Pro", Media),
            ("Documents/Poster.sketch", Media),
            ("Library/Application Support/MobileSync/Backup", DeviceBackup),
            ("Library/Containers/com.utmapp.UTM", VirtualMachine),
            ("Parallels/Win.pvm", VirtualMachine),
            ("Documents/VMs/Arch.utm", VirtualMachine),
            ("Virtual Machines.localized/Ubuntu.vmwarevm", VirtualMachine),
            ("VirtualBox VMs/Debian", VirtualMachine),
            ("Library/Containers/com.docker.docker/Data/vms", VirtualMachine),
            ("Library/Group Containers/HUAQ24HBR6.dev.orbstack", VirtualMachine),
            (".orbstack", VirtualMachine),
            (".android/avd", VirtualMachine),
            ("Library/Application Support/Postgres", Database),
            ("Projects/app/.git", SourceCode),
            ("Projects/app/.git/objects", SourceCode),
            ("Projects/nb/.ipynb_checkpoints", SourceCode),
            ("Library/Developer/Xcode/Archives", SourceCode),
            ("Documents/Vault/.obsidian", Notes),
            ("Library/Group Containers/9K33E3U3T4.net.shinyfrog.bear", Notes),
            ("Library/Application Support/Notion/notion.db", Notes),
            ("Library/Containers/com.evernote.Evernote", Notes),
            ("Library/Group Containers/5U8NS4GX82.dayoneapp2", Notes),
            ("Library/Application Support/Steam/userdata", GameSaves),
            ("Library/Application Support/minecraft/saves", GameSaves),
            ("Library/Application Support/OpenEmu", GameSaves),
            ("Library/Application Support/unity.Studio.Game", GameSaves),
            ("Library/Application Support/Studio/Game/Saves", GameSaves),
            ("Library/Mobile Documents/com~apple~CloudDocs", CloudSync),
            ("Library/CloudStorage/Dropbox", CloudSync),
            ("Library/CloudStorage/OneDrive-Personal", CloudSync),
            ("Library/Application Support/Google/DriveFS", CloudSync),
            ("Dropbox", CloudSync),
            ("Google Drive", CloudSync),
            ("OneDrive - Contoso", CloudSync),
        ] {
            assert_eq!(category(rel), Some(cat), "{rel} -> {:?}", v(rel));
        }
        assert_eq!(
            static_verdict(Path::new("/opt/homebrew/var/postgresql@16")),
            protected(Database, "inside protected data", false)
        );
        assert!(matches!(static_verdict(Path::new("/usr/local/var/mysql")), Verdict::Protected { .. }));
    }

    #[test]
    fn allowlisted_cache_leaves_inside_protected_roots_are_allowed() {
        for rel in [
            "Library/Application Support/Signal/Cache",
            "Library/Application Support/Slack/Cache",
            "Library/Application Support/Slack/Code Cache",
            "Library/Application Support/Slack/GPUCache",
            "Library/Application Support/discord/Cache",
            "Library/Application Support/discord/Code Cache",
            "Library/Application Support/Microsoft/Teams/Cache",
            "Library/Application Support/Microsoft/Teams/Application Cache",
            "Library/Application Support/Microsoft/Teams/logs",
            "Library/Messages/StickerCache",
            "Library/Messages/Caches/Previews/Attachments",
            "Library/Containers/com.tencent.xinWeChat/Data/Library/Caches",
            "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
            "Library/Containers/com.microsoft.Word/Data/tmp",
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/CacheClip",
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/Cache",
            "Library/Application Support/Adobe/Common/Media Cache Files",
            "Library/Application Support/Notion/Cache",
            "Library/Application Support/com.bohemiancoding.sketch3/cache",
            "Library/Application Support/Steam/appcache",
            "Library/Application Support/Steam/steamapps/shadercache",
            "Library/Application Support/minecraft/logs",
            "Library/Application Support/AddressBook/Sources/ABC/Photos.cache",
            "Library/Developer/Xcode/UserData/IB Support",
            ".aws/cli/cache",
            ".kube/http-cache",
            ".docker/buildx/cache",
            "VirtualBox VMs/.cache",
        ] {
            assert_eq!(v(rel), Verdict::CacheLeaf, "{rel}");
        }
        assert_eq!(static_verdict(Path::new("/opt/homebrew/var/homebrew/locks")), Verdict::CacheLeaf);
    }

    #[test]
    fn data_next_to_cache_leaves_stays_protected() {
        for rel in [
            "Library/Application Support/Signal/sql",
            "Library/Application Support/Slack/Service Worker/CacheStorage",
            "Library/Application Support/Slack/Cache/.git",
            "Library/Containers/net.whatsapp.WhatsApp/Data/tmp",
            "Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram/stable/account-1/postbox/media",
            "Library/Messages/Attachments/ab",
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/Resolve Disk Database",
            "Library/Application Support/Steam/steamapps/common",
            "Library/Containers/com.apple.mail/Data/Library/Mail",
            "Library/Containers/com.microsoft.Word/Data/Library/Preferences/AutoRecovery",
            ".aws/credentials",
            ".kube/config",
            ".docker/config.json",
        ] {
            assert!(is_protected(rel), "{rel} -> {:?}", v(rel));
        }
    }

    #[test]
    fn folders_holding_protected_roots_are_refused() {
        for rel in [
            "",
            "Library",
            "Library/Application Support",
            "Library/Containers",
            "Library/Group Containers",
            "Library/Application Support/Microsoft",
            "Library/Application Support/Google",
            "Library/Developer",
            "Library/Developer/Xcode",
            "Library/Application Support/JetBrains",
            "Library/Application Support/JetBrains/IntelliJIdea2024.1",
            "Music",
            "Movies",
            "Documents",
            ".android",
            ".config",
        ] {
            assert!(
                matches!(v(rel), Verdict::Protected { contains: true, .. }),
                "{rel} -> {:?}",
                v(rel)
            );
        }
        for p in ["/", "/Users", "/opt", "/opt/homebrew", "/usr/local", "/Users/Shared"] {
            assert!(matches!(static_verdict(Path::new(p)), Verdict::Protected { contains: true, .. }), "{p}");
        }
    }

    #[test]
    fn ordinary_paths_are_not_protected() {
        for rel in [
            "Library/Caches/ru.keepcoder.Telegram",
            "Library/Caches/com.tinyspeck.slackmacgap",
            "Library/Caches/Google/Chrome",
            "Library/Logs/Signal",
            "Library/Application Support/Code/Cache",
            "Library/Application Support/Spotify/PersistentCache",
            "Library/Application Support/com.example.goneapp",
            "Library/Saved Application State/org.whispersystems.signal-desktop.savedState",
            "Library/Mail Downloads",
            "Library/Developer/Xcode/DerivedData",
            "Library/Developer/CoreSimulator/Caches",
            "Library/Containers/com.apple.stocks/Data/Library/Caches",
            "Downloads/Firefox.dmg",
            "Projects/app/node_modules",
            "Projects/app/target",
            ".npm/_cacache",
            ".cargo/registry/cache",
            ".gitignore",
            "Documents/notes.txt",
        ] {
            assert_eq!(v(rel), Verdict::NotProtected, "{rel}");
        }
    }

    #[test]
    fn bundle_categories_ignore_team_ids_and_group_prefixes() {
        assert_eq!(bundle_category("6N38VWS5BX.ru.keepcoder.Telegram"), Some(Messages));
        assert_eq!(bundle_category("group.net.whatsapp.WhatsApp.shared"), Some(Messages));
        assert_eq!(bundle_category("2BUA8C4S2C.com.1password"), Some(Credentials));
        assert_eq!(bundle_category("UBF8T346G9.OneDriveStandaloneSuite"), Some(CloudSync));
        assert_eq!(bundle_category("com.ledger.live"), Some(Wallet));
        assert_eq!(bundle_category("com.example.app"), None);
        assert_eq!(bundle_category("com.apple.Safari"), None);
    }

    #[test]
    fn refusals_name_their_category_and_code() {
        let err = check_general(&Path::new(H).join(".electrum")).unwrap_err();
        assert_eq!(err.category, Wallet);
        assert!(err.to_string().starts_with("protected_user_data: crypto wallet data"), "{err}");
        assert!(err.to_string().ends_with("/Users/tester/.electrum"));
        let err = check_general(&Path::new(H).join("Library/Safari")).unwrap_err();
        assert!(err.to_string().starts_with("browser_profile_data: "), "{err}");
    }

    #[test]
    fn contexts_decide_what_may_go() {
        let archives = Path::new(H).join("Library/Developer/Xcode/Archives");
        let docker = Path::new(H).join("Library/Containers/com.docker.docker/Data/vms");
        let icloud = Path::new(H).join("Library/Mobile Documents/com~apple~CloudDocs/a.pdf");
        let library = Path::new(H).join("Library");
        for p in [&archives, &docker, &icloud, &library] {
            assert!(check_general(p).is_err(), "{}", p.display());
            assert!(check(p, DeleteContext::Uninstall { bundle_id: "com.docker.docker" }).is_err());
        }
        assert!(check(&archives, DeleteContext::Confirmed).is_ok());
        assert!(check(&docker, DeleteContext::Confirmed).is_ok());
        assert!(check(&icloud, DeleteContext::Confirmed).is_err(), "cloud files never go");
        assert!(check(&library, DeleteContext::Confirmed).is_err());
    }

    #[test]
    fn traversal_is_refused() {
        assert!(is_protected("Library/Caches/../Messages"));
    }

    #[test]
    fn marker_folders_are_detected_on_disk() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let pg = root.join("data/pg");
        write_file(&pg.join("PG_VERSION"), 3);
        write_file(&pg.join("base/1/1259"), 10);
        let repo = root.join("code/app");
        write_file(&repo.join(".git/HEAD"), 10);
        write_file(&repo.join("node_modules/x/index.js"), 10);
        let vault = root.join("notes/Vault");
        write_file(&vault.join(".obsidian/app.json"), 10);
        write_file(&vault.join("Ideas/idea.md"), 10);
        let graph = root.join("notes/graph");
        write_file(&graph.join("logseq/config.edn"), 10);
        write_file(&graph.join("pages/a.md"), 10);
        let plain = root.join("plain");
        write_file(&plain.join("a/b.bin"), 10);

        for p in [&pg, &pg.join("base"), &repo, &vault, &vault.join("Ideas"), &graph, &graph.join("pages")] {
            assert!(check_general(p).is_err(), "{}", s(p));
        }
        assert!(check_general(&repo.join("node_modules")).is_ok(), "artifacts inside a repo stay prunable");
        assert!(check_general(&plain).is_ok());
        assert_eq!(scan_tree(&root.join("code"), 3, 100), TreeScan::Found(SourceCode));
        assert_eq!(scan_tree(&plain, 3, 100), TreeScan::Clean);
    }

    #[test]
    fn a_folder_holding_protected_data_splits_into_its_deletable_children() {
        let dir = workspace_tempdir();
        let root = canon(&dir).join("appdata");
        write_file(&root.join("wallet.dat"), 10);
        write_file(&root.join("blocks/blk00000.dat"), 10);
        write_file(&root.join("debug.log"), 10);
        let (ok, kept) = split(&root, DeleteContext::General, 2);
        let mut ok: Vec<String> = ok.iter().map(|p| s(p)).collect();
        ok.sort();
        assert_eq!(ok, vec![s(&root.join("blocks")), s(&root.join("debug.log"))]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].category, Wallet);
        assert!(kept[0].path.ends_with("wallet.dat"));
    }

    #[test]
    fn symlinks_into_protected_data_are_judged_by_their_target() {
        let dir = workspace_tempdir();
        let root = canon(&dir);
        let vm = root.join("vms/Linux.utm");
        write_file(&vm.join("Data/disk.qcow2"), 10);
        let link = root.join("innocent");
        std::os::unix::fs::symlink(&vm, &link).unwrap();
        assert!(check_general(&link).is_err());
    }

    #[test]
    fn fake_home_protected_data_is_refused_and_cache_leaves_allowed() {
        let dir = workspace_tempdir();
        let fx = fixtures::build(&canon(&dir).join("home"));
        for rel in fixtures::PROTECTED {
            assert!(check_general(&fx.path(rel)).is_err(), "{rel} should be refused");
        }
        for p in &fx.protected_dirs {
            assert!(check_general(p).is_err(), "{} should be refused", s(p));
        }
        for p in &fx.cache_leaves {
            let leaf = p.parent().unwrap();
            assert!(check_general(leaf).is_ok(), "{} should be allowed: {:?}", s(leaf), check_general(leaf));
        }
    }

    /// Every path a cleaner rule or a Pawtrol probe can produce must stay
    /// out of protected roots or land on an allowlisted cache leaf. Pawtrol
    /// categories flagged as user data are only cleaned after the user
    /// confirms their data loss, so they may reach protected data, but
    /// never cloud-synced files.
    #[test]
    fn no_cleaner_rule_or_pawtrol_probe_reaches_protected_data() {
        let mut bad = Vec::new();
        for rule in crate::commands::cleaner::rules::all_rules() {
            for raw in &rule.paths {
                let path = PathBuf::from(raw.replacen('~', H, 1).replace('*', "x"));
                match static_verdict(&path) {
                    Verdict::NotProtected | Verdict::CacheLeaf => {}
                    // Age-filtered rules delete children one by one; the
                    // runtime guard filters protected children.
                    Verdict::Protected { contains: true, .. } if rule.max_age_days.is_some() => {}
                    other => bad.push(format!("rule {} ({}): {:?}", rule.id, raw, other)),
                }
            }
        }
        for def in crate::commands::guardian::probes::CATEGORIES {
            for path in crate::commands::guardian::probes::paths_for(def.id, Path::new(H)) {
                match (static_verdict(&path), def.data_loss) {
                    (Verdict::NotProtected | Verdict::CacheLeaf, _) => {}
                    (Verdict::Protected { category, contains: false, .. }, Some(_))
                        if !matches!(category, CloudSync | UserData) => {}
                    (other, _) => bad.push(format!("probe {} ({}): {:?}", def.id, path.display(), other)),
                }
            }
        }
        assert!(bad.is_empty(), "paths reaching protected data: {bad:#?}");
    }
}
