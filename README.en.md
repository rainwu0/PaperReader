<h1 align="center">PaperReader</h1>

<p align="center">A literature reader for researchers: read paper PDFs on your phone, highlight, take notes, and pick up where you left off.</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="Platform: Android" src="https://img.shields.io/badge/platform-Android-3DDC84.svg">
  <img alt="Status: pre-release" src="https://img.shields.io/badge/status-pre--release-orange.svg">
</p>

<p align="center"><a href="README.md">繁體中文</a> · <b>English</b></p>

---

PaperReader keeps your library on your own device as ordinary folders and files. A single phone is enough: no computer, server or cloud account is required. Features that need an outside service, such as online translation, use your own account and are added on top.

> **Project status**: preview. During 0.x, features and file formats may change.

## Table of Contents

- [Features](#features)
- [Not available](#not-available)
- [Install](#install)
- [Build from Source](#build-from-source)
- [Privacy](#privacy)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

## Features

| Feature | What it does |
|---|---|
| Add papers | Add one or more PDFs through the system file picker, or share them in from another app; title, authors and year are read from the PDF; duplicates are not added twice; while idle, the DOI and arXiv link are filled in on the device. Bulk import: a folder or zip with a manifest (`paperreader-import.json`) brings in existing papers, translations and bibliographic data in one go |
| Find papers | Search by title, author or year; the "Full text" section under the search box finds words in the original, the translation, your comments and notes, and tapping a result opens that page with the passage boxed (offline); sort; filter by reading status, has translation, has key notes or has notes; organize papers into groups; long-press to select several |
| Read | Switch between the original PDF and the translated PDF on the same page and position; double-tap to zoom to a column; go to page, outline, dark mode; a toolbox that can be minimized, floated or docked |
| Highlights and notes | Highlight selected text and add a short comment; draw on the page with the brush; write notes for the whole paper in Markdown. Notes are saved as Markdown files that note-taking apps such as Obsidian can open directly |
| Reading history | Records the page you reached automatically; each paper shows unread, in progress or read; one tap reopens the last page you read |
| Translation | Use your own API key with Claude, OpenAI, Gemini, an OpenAI-compatible endpoint, DeepL or Google Cloud Translation; or download offline models (Firefox Translations models, English to Traditional or Simplified Chinese) and translate on the device. Generates a translated PDF with the same layout that you can highlight; you can also overlay the translation on the original page, or read it as reflowed text and edit it in place. Prompt presets, a glossary, primary and fallback services, and side-by-side comparison of services. An optional layout model splits paragraphs more accurately |
| AI key notes | Use your own chat-model account to summarize a paper by section; each point carries the page number in the original, and tapping it takes you there to check. Saved as a Markdown file |
| Links between papers | Extracts each paper's reference list; tap a citation such as "[12]" in the text to see that entry and open it if it is in your library; the paper info lists which papers in your library this one cites and which cite it, and you can reject or pick a link that is wrong. All matching happens on the device, offline |
| Export and print | PDF with highlights and drawings, note files, translation (Markdown), bibliography files (BibTeX, RIS, CSL-JSON), citations (APA, IEEE, Chicago, MLA) |
| Backups and sync packages | Pack the whole library into one backup file; between two devices, move a sync package by hand and merge it on import, choosing yourself where both sides changed |
| Links to a page | `paperreader://` links open a specific page of a specific paper |
| Trash | Removed papers and materials go to the trash first and can be restored |

## Not available

Not available:

- Desktop version
- Automatic sync: a sync folder, WebDAV

## Install

Installable packages are on this repository's Releases page, starting with 0.1.0. Most phones need `PaperReader-<version>-arm64.apk` (for example `PaperReader-0.1.1-arm64.apk`; a fix release has a fourth number, such as `PaperReader-0.1.1.1-arm64.apk`); use the `x86_64` one for the emulator. Each release includes `SHA256SUMS.txt` for checking the download. Downloading, checking, installing and first use are described in [Installation and first use](docs/安裝與第一次使用.md) (Traditional Chinese).

Screenshots are in the [user guides](#documentation).

## Build from Source

The app is built with Tauri 2: the core and shell are in Rust, the interface in React and TypeScript, and Android platform features in Kotlin. It has only been built on Windows so far.

**Prerequisites**

- Rust 1.93 or later, with the targets `aarch64-linux-android` (devices) and `x86_64-linux-android` (emulator)
- Node.js and npm
- JDK, Android SDK and NDK, with `JAVA_HOME`, `ANDROID_HOME` and `NDK_HOME` set
- On Windows, turn on Developer Mode (the last step of the Tauri build creates symbolic links)

**Build**

```powershell
cd app
npm ci
npm run build
npx tauri android build --apk --debug --target aarch64   # use x86_64 for the emulator
```

The APK is written to `app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/`.

A release build needs your own signing key: copy `app/src-tauri/gen/android/keystore.properties.example` to `keystore.properties`, fill it in, then run `pwsh -NoProfile -File app/scripts/release-android.ps1`. It builds one APK each for arm64 and x86_64, verifies the signatures, and puts the APKs and `SHA256SUMS.txt` in `app/target/release-apk/`. Without `keystore.properties` the release APK is unsigned and cannot be installed directly. Keeping the repository under a path with only ASCII characters avoids path restrictions in the Android tools.

**Checks**

```powershell
pwsh -NoProfile -File app/scripts/check.ps1
```

Runs type checking, linting, interface tests, the build, rustfmt, clippy, cargo-deny and all tests, in that order (cargo-deny must be installed first). See [`app/README.md`](app/README.md) for how the code is organized.

## Privacy

- **Core features work offline**: adding, reading, highlighting, notes, reading history, export, backup, sync packages, organizing the library and links between papers all happen on the device.
- **Only features you turn on connect to the network**:
  - Online translation and AI key notes send the paper's text to the service you chose. Before each request the app shows where it goes and what is sent, and sends nothing without your consent. You can turn off the prompt in settings.
  - Model downloads: the offline translation model (about 34 MB) and the layout model (about 124 MB) are downloaded only with your consent. They then run on the device and no paper content is sent.
  - "Test service" in settings contacts the translation service you entered with one fixed sentence.
- **No account and no usage data collection.** Translation API keys are stored encrypted on the device and use your own account and quota.
- Your papers, notes and history are files on your device, and a backup file takes all of them with you. See the [privacy statement](docs/privacy.en.md) for details.

## Documentation

Except for the privacy statement, the following documents are currently in Traditional Chinese only.

**User guides** (in [`docs/`](docs/))

| Document | Contents |
|---|---|
| [Installation and first use](docs/安裝與第一次使用.md) | Installing, where the library lives (the "All files access" trade-off), adding your first paper |
| [Reading and marking up](docs/閱讀與劃記.md) | The toolbox, switching between original and translation, translation overlaid on the page, double-tap zoom, highlights, comments, brush, go to page, outline, citation cards and links between papers |
| [Notes and Obsidian](docs/筆記與Obsidian.md) | Where note files live, what they look like, sharing them with Obsidian |
| [Translation and key notes](docs/翻譯與重點筆記.md) | API keys for each service, costs, asking before sending, offline translation and the layout model, prompts and glossary, key notes |
| [Backups and sync packages](docs/備份與同步包.md) | Backup files vs. sync packages, moving between two devices, resolving conflicts, known limitations |
| [FAQ](docs/常見問題.md) | Network use, uninstalling, problems found by the library check, translation errors and more |
| [Design principles](docs/設計理念.md) | Data as files, complete on one device, replaceable generation methods, plugins |
| [Library format](docs/格式/文獻庫.md) | Folders, `paper.json`, materials, reading history, note files, `ink.json`, `references.json`, and compatibility rules (may change during 0.x) |
| [Privacy](docs/privacy.en.md) ([繁體中文](docs/privacy.md)) | Where your data lives, when the app goes online, what it sends |

**Code documents**

| Document | Contents |
|---|---|
| [`app/README.md`](app/README.md) | Code organization, checks and tool versions |

## Contributing

Issues for bug reports and suggestions are welcome on GitHub. Before submitting code, run the checks above; commit messages follow [Conventional Commits](https://www.conventionalcommits.org/).

## License

Licensed under the [Apache License 2.0](LICENSE). Third-party components and their licenses are listed in the app's About page.
