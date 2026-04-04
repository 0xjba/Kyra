<div align="center">
  <img src="AppIcon.png" width="180" alt="Kyra" />

  <h1>Kyra</h1>

  <p><strong>Nine lives for your storage.</strong></p>

  <p>A fast, beautiful macOS app that cleans junk, reclaims disk space,<br />and keeps your Mac running smoothly.</p>

  <p>
    <a href="https://github.com/0xjba/Kyra/releases/latest"><img src="https://img.shields.io/github/v/release/0xjba/Kyra?style=flat-square&color=blue&label=download" alt="Download" /></a>
    <img src="https://img.shields.io/badge/platform-macOS-000?style=flat-square&logo=apple&logoColor=white" alt="macOS" />
    <img src="https://img.shields.io/badge/Apple_Silicon-M1%2FM2%2FM3%2FM4-000?style=flat-square&logo=apple&logoColor=white" alt="Apple Silicon" />
    <a href="https://github.com/0xjba/Kyra/blob/main/LICENSE"><img src="https://img.shields.io/github/license/0xjba/Kyra?style=flat-square" alt="License" /></a>
    <img src="https://img.shields.io/github/stars/0xjba/Kyra?style=flat-square&color=yellow" alt="Stars" />
  </p>

  <p>
    <a href="https://github.com/0xjba/Kyra/releases/latest">Download</a> &nbsp;&middot;&nbsp;
    <a href="https://github.com/0xjba/Kyra/blob/main/CHANGELOG.md">Changelog</a> &nbsp;&middot;&nbsp;
    <a href="https://github.com/0xjba/Kyra/blob/main/LICENSE">License</a>
  </p>
</div>

<br />

<p align="center"><img src="src/assets/screenshot.jpg" width="720" alt="Kyra" /></p>

## Features

- **Clean** — Scan and remove system caches, user caches, logs, browser data, and orphaned app files
- **Prune** — Find and bulk-delete developer artifacts like `node_modules`, `target`, `__pycache__`, and `.build` across all your projects
- **Installers** — Locate leftover `.dmg`, `.pkg`, and `.iso` files sitting in your Downloads
- **Uninstall** — Fully remove apps along with their support files, caches, and preferences
- **Optimize** — Run system maintenance tasks like flushing DNS, rebuilding LaunchServices, and vacuuming databases
- **Analyze** — Treemap disk visualizer, large file finder, and duplicate file detector
- **Status** — Real-time CPU, memory, disk, and network monitoring
- **Ask AI** — Not sure if something is safe to delete? Ask AI opens ChatGPT with your scan results for a second opinion

## Install

1. Download the latest `.dmg` from [**Releases**](https://github.com/0xjba/Kyra/releases/latest)
2. Open the `.dmg` and drag Kyra to your Applications folder
3. Launch Kyra — the onboarding will guide you through setup

> [!NOTE]
> Kyra is not yet notarized by Apple. On first launch macOS may show "Kyra is damaged." To fix this, go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. Or run `xattr -cr /Applications/Kyra.app` in Terminal.

## Build from source

```bash
git clone https://github.com/0xjba/Kyra.git
cd Kyra
npm install
npm run tauri dev
```

Requires [Node.js](https://nodejs.org/) 22+ and [Rust](https://rustup.rs/).

<details>
<summary>Production build</summary>

```bash
npm run tauri build -- --target aarch64-apple-darwin
```

</details>

## Tech

<p>
  <img src="https://img.shields.io/badge/Tauri-24C8D8?style=flat-square&logo=tauri&logoColor=white" alt="Tauri" />
  <img src="https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/React-61DAFB?style=flat-square&logo=react&logoColor=black" alt="React" />
  <img src="https://img.shields.io/badge/TypeScript-3178C6?style=flat-square&logo=typescript&logoColor=white" alt="TypeScript" />
</p>

## License

[MIT](LICENSE) — made by [Jobin Ayathil](https://github.com/0xjba)
