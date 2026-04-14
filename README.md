<div align="center">
  <img src="AppIcon.png" width="180" alt="Kyra" />

  <h1>Kyra</h1>

  <p><strong>Nine lives for your storage.</strong></p>

  <p>
    <a href="https://github.com/0xjba/Kyra/releases/latest"><img src="https://img.shields.io/github/v/release/0xjba/Kyra?style=flat-square&color=blue&label=download" alt="Download" /></a>
    <img src="https://img.shields.io/badge/platform-macOS-000?style=flat-square&logo=apple&logoColor=white" alt="macOS" />
    <img src="https://img.shields.io/badge/Apple_Silicon-M1%2FM2%2FM3%2FM4%2FM5-000?style=flat-square&logo=apple&logoColor=white" alt="Apple Silicon" />
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

Kyra is a free, open-source macOS app that hunts down the junk hoarding your disk space: caches, logs, orphaned files, forgotten installers, bloated dev folders. Just a cat that keeps your Mac clean.

**270+ apps and services** cleaned safely. **16 browsers** supported. **56 developer artifact types** across 15+ languages. **24 system optimization tasks.** All running locally on your machine.

## Features

| | | |
|---|---|---|
| **Clean** | System & browser caches, logs, orphaned app data | 270+ apps and services covered |
| **Prune** | `node_modules`, `target`, `__pycache__`, `.build` | 56 artifact types across 15+ languages |
| **Installers** | Leftover `.dmg`, `.pkg`, `.iso` in Downloads | Stop forgetting about them |
| **Uninstall** | Remove apps + all their hidden support files | Cleaner than dragging to Trash |
| **Optimize** | DNS flush, LaunchServices rebuild, DB vacuum | 24 maintenance tasks in one click |
| **Analyze** | Treemap visualizer, large files, duplicates | See where your space goes |
| **Status** | CPU, memory, disk, network in real time | Glanceable system monitor |
| **Ask AI** | Not sure what's safe to delete? | Opens ChatGPT with your scan |

## Install

1. Download the latest `.dmg` from [**Releases**](https://github.com/0xjba/Kyra/releases/latest)
2. Open the `.dmg` and drag Kyra to your Applications folder
3. Launch Kyra. The onboarding will guide you through setup

> Requires **macOS 11+** and **Apple Silicon** (M1/M2/M3/M4/M5).

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

[MIT](LICENSE). Made by [Jobin Ayathil](https://github.com/0xjba)
