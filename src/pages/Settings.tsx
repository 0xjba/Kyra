import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { SlidersHorizontal, Siren, ScanSearch, Bell, EyeOff, Database, X, type LucideIcon } from "lucide-react";
import { useSettingsStore } from "../stores/settingsStore";
import { useGuardianStore } from "../stores/guardianStore";
import {
  resetLifetimeStats,
  getStoragePath,
  getTotalBytesFreed,
  pickFolder,
  revealLogInFinder,
  saveSettings,
} from "../lib/tauri";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import { formatSize } from "../utils/format";
import { devicesLine, planLine } from "../utils/pawtrolAccount";
import SubscribeSheet from "../components/SubscribeSheet";
import RestoreSheet from "../components/RestoreSheet";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import "../styles/settings.css";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];

const LARGE_FILE_OPTIONS = [50, 100, 250, 500, 1000];
const SCAN_DEPTH_OPTIONS = [4, 6, 8, 10, 12];
const LOW_DISK_OPTIONS = [5, 10, 15, 20, 25];

type SectionId = "general" | "pawtrol" | "scanning" | "alerts" | "ignore" | "data";

const SECTIONS: { id: SectionId; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "pawtrol", label: "Pawtrol Pro", icon: Siren },
  { id: "scanning", label: "Scanning", icon: ScanSearch },
  { id: "alerts", label: "Alerts & Updates", icon: Bell },
  { id: "ignore", label: "Ignore List", icon: EyeOff },
  { id: "data", label: "Data", icon: Database },
];

function stepOption(options: number[], current: number, dir: 1 | -1): number {
  const idx = options.indexOf(current);
  const next = Math.max(0, Math.min(options.length - 1, (idx < 0 ? 0 : idx) + dir));
  return options[next];
}

function Row({ name, desc, children, mono }: { name: ReactNode; desc?: ReactNode; children?: ReactNode; mono?: boolean }) {
  return (
    <div className="st-row">
      <div className="st-row-info">
        <div className="st-row-name">{name}</div>
        {desc != null && desc !== "" && <div className={`st-row-desc${mono ? " st-mono" : ""}`}>{desc}</div>}
      </div>
      {children}
    </div>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className={`st-toggle${on ? " on" : ""}`}
      onClick={() => onChange(!on)}
    >
      <span className="st-toggle-knob" />
    </button>
  );
}

function Stepper({ value, onDec, onInc }: { value: string; onDec: () => void; onInc: () => void }) {
  return (
    <div className="st-stepper">
      <button type="button" className="st-stepper-btn" onClick={onDec} aria-label="Decrease">&minus;</button>
      <span className="st-stepper-value">{value}</span>
      <button type="button" className="st-stepper-btn" onClick={onInc} aria-label="Increase">+</button>
    </div>
  );
}

function Pill({ children, onClick, variant, disabled }: { children: ReactNode; onClick?: () => void; variant?: "primary" | "danger"; disabled?: boolean }) {
  return (
    <button
      type="button"
      className={`st-pill${variant ? ` st-pill-${variant}` : ""}`}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

function Section({ id, label, children }: { id: SectionId; label: string; children: ReactNode }) {
  return (
    <div className="st-section" data-sec={id}>
      <div className="st-section-label">{label}</div>
      <div className="st-card">{children}</div>
    </div>
  );
}

export default function Settings() {
  const navigate = useNavigate();
  const settings = useSettingsStore((s) => s.settings);
  const loaded = useSettingsStore((s) => s.loaded);
  const load = useSettingsStore((s) => s.load);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const setLargeFileThreshold = useSettingsStore((s) => s.setLargeFileThreshold);
  const setAnalyzeScanDepth = useSettingsStore((s) => s.setAnalyzeScanDepth);
  const setLaunchAtLogin = useSettingsStore((s) => s.setLaunchAtLogin);
  const setCheckForUpdates = useSettingsStore((s) => s.setCheckForUpdates);
  const setNotificationsEnabled = useSettingsStore((s) => s.setNotificationsEnabled);
  const setLowDiskThreshold = useSettingsStore((s) => s.setLowDiskThreshold);
  const setOnboardingCompleted = useSettingsStore((s) => s.setOnboardingCompleted);
  const addWhitelist = useSettingsStore((s) => s.addWhitelist);
  const removeWhitelist = useSettingsStore((s) => s.removeWhitelist);

  const license = useGuardianStore((s) => s.license);
  const deviceName = useGuardianStore((s) => s.deviceName);
  const checkLicense = useGuardianStore((s) => s.checkLicense);
  const account = useGuardianStore((s) => s.account);
  const accountError = useGuardianStore((s) => s.accountError);
  const loadAccount = useGuardianStore((s) => s.loadAccount);
  const manageSubscription = useGuardianStore((s) => s.manageSubscription);
  const setPawtrolTab = useGuardianStore((s) => s.setPawtrolTab);

  const [active, setActive] = useState<SectionId>("general");
  const [frame, setFrame] = useState(0);
  const [showInput, setShowInput] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [storagePath, setStoragePath] = useState("");
  const [totalFreed, setTotalFreed] = useState(0);
  const [statsReset, setStatsReset] = useState(false);
  const [sheet, setSheet] = useState<"subscribe" | "restore" | null>(null);
  const [openingManage, setOpeningManage] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "available" | "downloading" | "up-to-date">("idle");
  const [autoStartSynced, setAutoStartSynced] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    if (!loaded) load();
    getStoragePath().then((v) => { if (!cancelled) setStoragePath(v); }).catch(() => {});
    getTotalBytesFreed().then((v) => { if (!cancelled) setTotalFreed(v); }).catch(() => {});
    getVersion().then((v) => { if (!cancelled) setAppVersion(v); }).catch(() => {});
    return () => { cancelled = true; };
  }, [loaded, load]);

  useEffect(() => {
    checkLicense();
  }, [checkLicense]);

  useEffect(() => {
    if (license.active) loadAccount();
  }, [license.active, loadAccount]);

  const closeSheet = useCallback(() => setSheet(null), []);

  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % CAT_FRAMES.length), 220);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    if (loaded && !autoStartSynced) {
      isEnabled().then((enabled) => {
        if (enabled !== settings.launch_at_login) {
          setLaunchAtLogin(enabled);
        }
        setAutoStartSynced(true);
      }).catch(() => setAutoStartSynced(true));
    }
  }, [loaded, autoStartSynced, settings.launch_at_login, setLaunchAtLogin]);

  const jumpLock = useRef<{ id: SectionId; until: number } | null>(null);

  const sectionTop = (root: HTMLElement, el: HTMLElement) =>
    el.getBoundingClientRect().top - root.getBoundingClientRect().top + root.scrollTop;

  const jump = useCallback((id: SectionId) => {
    const root = scrollRef.current;
    const el = root?.querySelector<HTMLElement>(`[data-sec="${id}"]`);
    if (root && el) {
      jumpLock.current = { id, until: Date.now() + 800 };
      root.scrollTo({ top: Math.max(0, sectionTop(root, el) - 4), behavior: "smooth" });
    }
    setActive(id);
  }, []);

  const onScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    // Smooth scroll to a short trailing section can't bring it to the top; keep the clicked item active.
    if (jumpLock.current && Date.now() < jumpLock.current.until) return;
    jumpLock.current = null;
    const secs = Array.from(el.querySelectorAll<HTMLElement>("[data-sec]"));
    let cur = secs[0]?.dataset.sec as SectionId | undefined;
    secs.forEach((x) => { if (sectionTop(el, x) - el.scrollTop < 60) cur = x.dataset.sec as SectionId; });
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 2 && secs.length) {
      cur = secs[secs.length - 1].dataset.sec as SectionId;
    }
    if (cur) setActive((prev) => (prev === cur ? prev : cur!));
  }, []);

  if (!loaded) return null;

  const handleAutoStartToggle = async (enabled: boolean) => {
    try {
      if (enabled) {
        await enable();
      } else {
        await disable();
      }
      await setLaunchAtLogin(enabled);
    } catch {}
  };

  const handleManageSubscription = async () => {
    setOpeningManage(true);
    await manageSubscription();
    setOpeningManage(false);
  };

  const handleCheckForUpdate = async () => {
    setUpdateStatus("checking");
    try {
      const update = await check();
      if (update) {
        setUpdateStatus("available");
      } else {
        setUpdateStatus("up-to-date");
        setTimeout(() => setUpdateStatus("idle"), 3000);
      }
    } catch {
      setUpdateStatus("idle");
    }
  };

  const handleDownloadUpdate = async () => {
    setUpdateStatus("downloading");
    try {
      const update = await check();
      if (update) {
        await update.downloadAndInstall();
        await relaunch();
      }
    } catch {
      setUpdateStatus("idle");
    }
  };

  const handleAddPath = async () => {
    const trimmed = newPath.trim();
    if (trimmed && !settings.whitelist.includes(trimmed)) {
      await addWhitelist(trimmed);
    }
    setNewPath("");
    setShowInput(false);
  };

  const handleBrowse = async () => {
    const selected = await pickFolder();
    if (selected) {
      const trimmed = selected.replace(/\/$/, "");
      if (!settings.whitelist.includes(trimmed)) {
        await addWhitelist(trimmed);
      }
    }
  };

  const handleResetStats = async () => {
    await resetLifetimeStats();
    setTotalFreed(0);
    setStatsReset(true);
    setTimeout(() => setStatsReset(false), 2000);
  };

  const handleResetSettings = async () => {
    const defaults = {
      dry_run: false,
      whitelist: [],
      use_trash: false,
      large_file_threshold_mb: 100,
      analyze_scan_depth: 8,
      launch_at_login: false,
      check_for_updates: true,
      notifications_enabled: true,
      low_disk_threshold_gb: 10,
      onboarding_completed: false,
      pawtrol_enabled: true,
      pawtrol_login_prompted: settings.pawtrol_login_prompted,
    };
    try { await disable(); } catch {}
    await saveSettings(defaults);
    await load();
  };

  const openRules = () => {
    setPawtrolTab("rules");
    navigate("/guardian");
  };

  const link = (url: string) => (e: React.MouseEvent) => {
    e.preventDefault();
    openUrl(url).catch(console.error);
  };

  const versionLabel = appVersion ? `Kyra ${appVersion}` : "Kyra";
  const largeLabel = settings.large_file_threshold_mb >= 1000
    ? `${settings.large_file_threshold_mb / 1000} GB`
    : `${settings.large_file_threshold_mb} MB`;

  const updateDesc =
    updateStatus === "checking" ? "Checking for updates…" :
    updateStatus === "available" ? "A new version is available" :
    updateStatus === "downloading" ? "Downloading update…" :
    updateStatus === "up-to-date" ? "You're up to date" :
    `${versionLabel} is installed`;
  const updateLabel =
    updateStatus === "checking" ? "Checking…" :
    updateStatus === "downloading" ? "Installing…" :
    updateStatus === "up-to-date" ? "Up to date" :
    "Check now";

  return (
    <div className="st-container">
      <nav className="st-nav">
        {SECTIONS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className={`st-nav-item${active === id ? " active" : ""}`}
            onClick={() => jump(id)}
          >
            <Icon size={14} strokeWidth={2} className="st-nav-icon" />
            {label}
          </button>
        ))}
        <div className="st-nav-about">
          <img src={CAT_FRAMES[frame]} alt="" className="st-nav-cat" draggable={false} />
          <div className="st-nav-version">{versionLabel}</div>
          <div className="st-nav-tagline">Nine lives for your storage</div>
        </div>
      </nav>

      <div className="st-scroll" ref={scrollRef} onScroll={onScroll}>
        <Section id="general" label="General">
          <Row name="Launch at login" desc="Start Kyra when you log in">
            <Toggle label="Launch at login" on={settings.launch_at_login} onChange={handleAutoStartToggle} />
          </Row>
          <Row name="Move to Trash" desc="Send files to Trash instead of deleting them">
            <Toggle label="Move to Trash" on={settings.use_trash} onChange={setUseTrash} />
          </Row>
        </Section>

        <Section id="pawtrol" label="Pawtrol Pro">
          {license.active ? (
            <>
              {account && <Row name="Account" desc={account.email} />}
              {account === undefined && accountError && (
                <Row name="Account" desc={<span className="st-row-error">{accountError}</span>}>
                  <Pill onClick={loadAccount}>Retry</Pill>
                </Row>
              )}
              <Row name="Plan" desc={planLine(account ?? null, license.expires)} />
              {account && <Row name="Devices" desc={devicesLine(account)} />}
              <Row name="Pawtrol rules" desc="Schedule, low-space alerts and what it cleans">
                <Pill onClick={openRules}>Open</Pill>
              </Row>
              {account && (
                <Row
                  name="Manage subscription"
                  desc={accountError
                    ? <span className="st-row-error">{accountError}</span>
                    : "Change card or cancel on the secure billing page"}
                >
                  <Pill onClick={handleManageSubscription} disabled={openingManage}>
                    {openingManage ? "Opening…" : "Manage"}
                  </Pill>
                </Row>
              )}
            </>
          ) : (
            <>
              <Row name="Subscription" desc={`Keeps your ${deviceName || "Mac"} clean on its own · from $0.99/mo`}>
                <Pill variant="primary" onClick={() => setSheet("subscribe")}>Subscribe</Pill>
              </Row>
              <Row name="Already subscribed?" desc="Restore Pawtrol with the email you subscribed with">
                <Pill onClick={() => setSheet("restore")}>Restore</Pill>
              </Row>
            </>
          )}
        </Section>

        <Section id="scanning" label="Scanning">
          <Row name="Large file threshold" desc="Flag files bigger than">
            <Stepper
              value={largeLabel}
              onDec={() => setLargeFileThreshold(stepOption(LARGE_FILE_OPTIONS, settings.large_file_threshold_mb, -1))}
              onInc={() => setLargeFileThreshold(stepOption(LARGE_FILE_OPTIONS, settings.large_file_threshold_mb, 1))}
            />
          </Row>
          <Row name="Analyze scan depth" desc="Folder levels to map">
            <Stepper
              value={`${settings.analyze_scan_depth} levels`}
              onDec={() => setAnalyzeScanDepth(stepOption(SCAN_DEPTH_OPTIONS, settings.analyze_scan_depth, -1))}
              onInc={() => setAnalyzeScanDepth(stepOption(SCAN_DEPTH_OPTIONS, settings.analyze_scan_depth, 1))}
            />
          </Row>
        </Section>

        <Section id="alerts" label="Alerts & Updates">
          <Row name="Notifications" desc="Low disk space and update alerts">
            <Toggle label="Notifications" on={settings.notifications_enabled} onChange={setNotificationsEnabled} />
          </Row>
          {license.active ? (
            <Row name="Low disk space alert" desc="Managed by Pawtrol">
              <Pill onClick={openRules}>View rules</Pill>
            </Row>
          ) : (
            <Row name="Low disk space alert" desc="Warn when free space drops below">
              <Stepper
                value={`${settings.low_disk_threshold_gb} GB`}
                onDec={() => setLowDiskThreshold(stepOption(LOW_DISK_OPTIONS, settings.low_disk_threshold_gb, -1))}
                onInc={() => setLowDiskThreshold(stepOption(LOW_DISK_OPTIONS, settings.low_disk_threshold_gb, 1))}
              />
            </Row>
          )}
          <Row name="Check for updates" desc="Automatically on launch">
            <Toggle label="Check for updates" on={settings.check_for_updates} onChange={setCheckForUpdates} />
          </Row>
          <Row name="Software update" desc={updateDesc}>
            {updateStatus === "available" ? (
              <Pill variant="primary" onClick={handleDownloadUpdate}>Update</Pill>
            ) : (
              <Pill
                onClick={handleCheckForUpdate}
                disabled={updateStatus === "checking" || updateStatus === "downloading"}
              >
                {updateLabel}
              </Pill>
            )}
          </Row>
        </Section>

        <Section id="ignore" label="Ignore List">
          {settings.whitelist.map((path) => (
            <Row key={path} name={<span className="st-path" title={path}>{path}</span>}>
              <button
                type="button"
                className="st-path-remove"
                onClick={() => removeWhitelist(path)}
                aria-label={`Remove ${path}`}
              >
                <X size={12} strokeWidth={2.25} />
              </button>
            </Row>
          ))}
          {showInput ? (
            <div className="st-row">
              <input
                type="text"
                className="st-input"
                placeholder="/path/to/protect"
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleAddPath();
                  if (e.key === "Escape") {
                    setShowInput(false);
                    setNewPath("");
                  }
                }}
                autoFocus
              />
              <Pill onClick={() => { setShowInput(false); setNewPath(""); }}>Cancel</Pill>
              <Pill variant="primary" onClick={handleAddPath}>Add</Pill>
            </div>
          ) : (
            <Row name="Add a folder" desc="Kyra won't scan or clean anything inside it">
              <div className="st-pill-group">
                <Pill onClick={() => setShowInput(true)}>Type path…</Pill>
                <Pill onClick={handleBrowse}>Choose…</Pill>
              </div>
            </Row>
          )}
        </Section>

        <Section id="data" label="Data">
          <Row
            name="Lifetime stats"
            desc={totalFreed > 0 ? `${formatSize(totalFreed)} reclaimed` : "Nothing reclaimed yet"}
          >
            <Pill onClick={handleResetStats} disabled={statsReset}>{statsReset ? "Reset ✓" : "Reset"}</Pill>
          </Row>
          <Row name="Storage location" desc={storagePath || "—"} mono />
          <Row name="Export logs" desc="Reveal operation logs in Finder">
            <Pill onClick={() => revealLogInFinder()}>Reveal</Pill>
          </Row>
          <Row name="Replay onboarding" desc="See the welcome tour again">
            <Pill onClick={() => { navigate("/"); setOnboardingCompleted(false); }}>Replay</Pill>
          </Row>
          <Row name="Reset all settings" desc="Restore every setting to its default">
            <Pill variant="danger" onClick={handleResetSettings}>Reset</Pill>
          </Row>
        </Section>

        <div className="st-footer">
          <a href="#" onClick={link("https://github.com/0xjba/Kyra")}>GitHub</a>
          <a href="#" onClick={link("https://github.com/0xjba/Kyra/blob/main/CHANGELOG.md")}>Changelog</a>
          <a href="#" onClick={link("https://github.com/0xjba/Kyra/blob/main/LICENSE")}>License</a>
          <span className="st-footer-dev">Developed by Jobin Ayathil</span>
        </div>
      </div>

      <SubscribeSheet open={sheet === "subscribe"} onClose={closeSheet} />
      <RestoreSheet open={sheet === "restore"} onClose={closeSheet} />
    </div>
  );
}
