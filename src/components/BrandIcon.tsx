import brandIcons from "../data/brand-icons.json";

const icons = brandIcons as Record<string, { path: string; hex: string }>;

/* Rule ID → brand icon key */
const RULE_BRAND: Record<string, string> = {
  // Browsers
  safari_cache: "safari", chrome_cache: "chrome", firefox_cache: "firefox",
  brave_cache: "brave", opera_cache: "opera", vivaldi_cache: "vivaldi",
  chromium_cache: "chrome",
  // Communication
  comm_discord: "discord", comm_zoom: "zoom", comm_telegram: "telegram",
  comm_whatsapp: "whatsapp", comm_wechat: "wechat", comm_signal: "signal",
  // Media
  media_spotify: "spotify", media_vlc: "vlc", media_obs: "obsstudio",
  media_plex: "plex",
  // Design
  design_figma: "figma", design_sketch: "sketch", design_blender: "blender",
  // AI Tools
  ai_claude_desktop: "anthropic", ai_cursor: "cursor",
  // Notes
  notes_notion: "notion", notes_obsidian: "obsidian", notes_evernote: "evernote",
  notes_todoist: "todoist", notes_linear: "linear",
  // Gaming
  game_steam: "steam", game_epic: "epicgames",
  // Utilities
  util_homebrew: "homebrew", util_raycast: "raycast", util_1password: "onepassword",
  // Dev — JS/TS ecosystem
  dev_npm_cache: "npm", dev_npm_logs: "npm", dev_npm_prebuilds: "npm",
  dev_yarn_cache: "yarn", dev_pnpm_store: "pnpm", dev_bun_cache: "bun",
  dev_deno_cache: "deno", dev_eslint_cache: "eslint", dev_prettier_cache: "prettier",
  dev_typescript_cache: "typescript", dev_turbo_cache: "turborepo",
  dev_electron_cache: "electron", dev_node_gyp: "npm",
  // Dev — Rust
  dev_cargo_registry: "rust", dev_cargo_git: "rust", dev_cargo_src: "rust",
  dev_rustup_downloads: "rust", dev_sccache: "rust",
  // Dev — Python
  dev_pip_cache: "python", dev_conda_packages: "python", dev_poetry_cache: "python",
  dev_pyenv_cache: "python", dev_uv_cache: "python", dev_pre_commit: "python",
  dev_ruff_cache: "python", dev_mypy_cache: "python", dev_jupyter_runtime: "python",
  // Dev — Go
  dev_go_mod_cache: "go", dev_go_build: "go",
  // Dev — JVM
  dev_gradle_cache: "gradle", dev_maven_cache: "maven",
  // Dev — iOS/macOS
  dev_cocoapods_cache: "cocoapods", dev_flutter_cache: "flutter",
  dev_xcode_derived: "xcode", dev_xcode_simulators: "xcode",
  dev_xcode_archives: "xcode", dev_xcode_device_support: "xcode",
  dev_xcode_docsets: "xcode",
  // Dev — Ruby/PHP
  dev_rubygems_cache: "rubygems", dev_composer_cache: "php",
  // Dev — Cloud/Infra
  dev_kubernetes_cache: "kubernetes", dev_gcloud_logs: "googlecloud",
  dev_android_cache: "android",
  // Dev — AI/ML
  dev_huggingface: "huggingface", dev_tensorflow_cache: "tensorflow",
  dev_torch_cache: "pytorch", dev_wandb_cache: "pytorch",
  // Dev — Misc
  dev_docker_cache: "docker", dev_docker_buildx: "docker",
  dev_jetbrains_cache: "jetbrains",
  dev_vscode_cache: "typescript", // VS Code uses TS icon as fallback if no app icon
};

interface BrandIconProps {
  ruleId: string;
  size?: number;
}

export function getBrandIcon(ruleId: string): { path: string; hex: string } | null {
  const key = RULE_BRAND[ruleId];
  if (!key) return null;
  return icons[key] || null;
}

export default function BrandIcon({ ruleId, size = 18 }: BrandIconProps) {
  const icon = getBrandIcon(ruleId);
  if (!icon) return null;

  // For very dark icons (black), lighten them for dark theme
  const color = icon.hex === "000000" || icon.hex === "191919" || icon.hex === "313131" || icon.hex === "302E31"
    ? "rgba(255, 255, 255, 0.7)"
    : `#${icon.hex}`;

  // Render inside a rounded-square container matching macOS app icon style
  const padding = Math.round(size * 0.18); // ~18% padding so the glyph doesn't touch edges
  const innerSize = size - padding * 2;

  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.22), // match macOS app icon radius
        background: "rgba(255, 255, 255, 0.06)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      <svg
        width={innerSize}
        height={innerSize}
        viewBox="0 0 24 24"
        fill={color}
      >
        <path d={icon.path} />
      </svg>
    </div>
  );
}
