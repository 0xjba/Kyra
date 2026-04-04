<p align="center">
  <img src="Logo.png" width="80" alt="Kyra" />
</p>

<h1 align="center">Kyra</h1>

<p align="center">
  <strong>Nine lives for your storage.</strong><br />
  A fast, beautiful macOS app to clean junk, reclaim disk space, and keep your Mac running smoothly.
</p>

<p align="center">
  <a href="https://github.com/0xjba/Kyra/releases/latest">Download</a> &nbsp;&middot;&nbsp;
  <a href="https://github.com/0xjba/Kyra/blob/main/CHANGELOG.md">Changelog</a> &nbsp;&middot;&nbsp;
  <a href="https://github.com/0xjba/Kyra/blob/main/LICENSE">License</a>
</p>

---

## What is Kyra?

Kyra is a native macOS app that helps you find and remove files you don't need — system caches, old logs, leftover installers, forgotten developer artifacts, and more. It's built with Tauri and Rust, so it's lightweight and fast.

## Features

- **Clean** — Scan and remove system caches, user caches, logs, browser data, and orphaned app files
- **Prune** — Find and bulk-delete developer artifacts like `node_modules`, `target`, `__pycache__`, and `.build` across all your projects
- **Installers** — Locate leftover `.dmg`, `.pkg`, and `.iso` files sitting in your Downloads
- **Uninstall** — Fully remove apps along with their support files, caches, and preferences
- **Optimize** — Run system maintenance tasks like flushing DNS, rebuilding LaunchServices, and vacuuming databases
- **Analyze** — Treemap disk visualizer, large file finder, and duplicate file detector
- **Status** — Real-time CPU, memory, disk, and network monitoring
- **Ask AI** — Not sure if something is safe to delete? Ask AI opens ChatGPT with your scan results for a second opinion

## Requirements

- macOS 13 Ventura or later
- Apple Silicon (M1 / M2 / M3 / M4)

## Install

1. Download the latest `.dmg` from [Releases](https://github.com/0xjba/Kyra/releases/latest)
2. Open the `.dmg` and drag Kyra to your Applications folder
3. Launch Kyra — the onboarding will guide you through granting Full Disk Access

> **Note:** Kyra is not yet notarized by Apple. On first launch, you may need to right-click the app and select "Open", then click "Open" again in the dialog.

## Build from source

```bash
# Clone
git clone https://github.com/0xjba/Kyra.git
cd Kyra

# Install dependencies
npm install

# Run in development
npm run tauri dev

# Build for production
npm run tauri build -- --target aarch64-apple-darwin
```

Requires [Node.js](https://nodejs.org/) 22+ and [Rust](https://rustup.rs/).

## License

[MIT](LICENSE) — made by [Jobin Ayathil](https://github.com/0xjba)
