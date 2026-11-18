<h1 align="center">PaperReader</h1>

<p align="center">給研究者的文獻閱讀器：在手機上讀論文 PDF、劃線、寫筆記，自動記下讀到哪裡。</p>

<p align="center">
  <a href="LICENSE"><img alt="授權：Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="平台：Android" src="https://img.shields.io/badge/platform-Android-3DDC84.svg">
  <img alt="狀態：開發中" src="https://img.shields.io/badge/status-pre--release-orange.svg">
</p>

<p align="center"><b>繁體中文</b> · <a href="README.en.md">English</a></p>

---

PaperReader 把文獻庫放在你自己的裝置上，存成一般的資料夾與檔案。只有一支手機、沒有電腦也沒有雲端帳號，就能完整使用；翻譯等需要外部服務的功能，用你自己的帳號另外加上。

> **專案狀態**：預覽版。0.x 期間，功能與檔案格式都可能變動。

## 目錄

- [特色](#特色)
- [目前沒有的功能](#目前沒有的功能)
- [安裝](#安裝)
- [從原始碼建置](#從原始碼建置)
- [隱私](#隱私)
- [文件](#文件)
- [參與](#參與)
- [授權](#授權)

## 特色

| 功能 | 內容 |
|---|---|
| 加入文獻 | 從系統選檔一次加入一份或多份 PDF，或從別的應用分享進來；自動讀出篇名、作者與年份；同一份不重複加入；閒著時在手機上補 DOI 與 arXiv 網址。整批匯入：一個資料夾或 zip 加一份清單（`paperreader-import.json`），把既有的論文、譯本與書目一次收進來 |
| 找文獻 | 依篇名、作者、年份搜尋；搜尋框下面的「內文」一節找原文、譯文、你的短評與筆記裡的字句，點了開到那一頁並框出來（不連網）；排序；依閱讀狀態、有譯本、有重點筆記、有筆記篩選；用群組分類；長按多選 |
| 閱讀 | 原文 PDF 與譯本 PDF 切換時停在同一頁同一處；雙擊放大到一欄的寬度；跳頁、大綱、深色；工具箱可縮小、浮動或停靠 |
| 劃線與筆記 | 選字劃線、寫短評；用筆刷在頁面上畫；筆記窗格寫整篇的想法（Markdown）。筆記存成 Markdown 檔，Obsidian 等筆記工具可以直接開 |
| 閱讀紀錄 | 自動記下讀到哪一頁；每篇看得出未讀、讀到一半、已讀；打開應用一次點擊回到上次那一頁 |
| 翻譯 | 用自己的金鑰接 Claude、OpenAI、Gemini、OpenAI 相容端點、DeepL、Google Cloud Translation；或下載離線模型（Firefox 翻譯的模型，英譯繁中、簡中）在裝置上翻。產生版面相同、可劃線的譯本 PDF，也可以疊在原頁上看，或看排成手機好讀的譯文、就地修改。提示詞預設組、術語庫、主要與備用服務、並排比較服務。選用下載的版面模型讓分段更準 |
| AI 重點筆記 | 用自己的對話模型帳號，依區段整理一篇的重點；每一點附原文頁碼，點了回到那一頁核對。存成 Markdown 檔 |
| 文獻之間的連結 | 抽出每篇的參考文獻；點正文的「[12]」看那一條，庫裡有就直接開啟；文獻資訊列出這篇引了庫裡哪幾篇、庫裡哪幾篇引了它，連錯了可以否決或指定。全部在手機上比對，不連網 |
| 匯出與列印 | 帶劃線與筆跡的 PDF、筆記檔、譯文（Markdown）、書目檔（BibTeX、RIS、CSL-JSON）、引用格式（APA、IEEE、Chicago、MLA） |
| 備份與同步包 | 整個文獻庫打包成一個備份檔；兩台裝置之間用同步包手動搬運，匯入時合併，兩邊都改過的由你選 |
| 連到某一頁 | `paperreader://` 連結直接開到某一篇的某一頁 |
| 回收區 | 移除的文獻與材料先放進回收區，可以還原 |

## 目前沒有的功能

目前沒有：

- 電腦版
- 自動同步：同步資料夾、WebDAV

## 安裝

安裝檔放在本版本庫的 Releases 頁，從 0.1.0 起提供：大部分手機下載 `PaperReader-版本-arm64.apk`（例如 `PaperReader-0.1.1-arm64.apk`；修正版多一段，如 `PaperReader-0.1.1.1-arm64.apk`），模擬器用 `x86_64` 那一個；每個版本附 `SHA256SUMS.txt` 可以核對。下載、核對、安裝與第一次使用的步驟見[安裝與第一次使用](docs/安裝與第一次使用.md)。

截圖在各份[使用者文件](#文件)裡。

## 從原始碼建置

應用以 Tauri 2 建成：核心與外殼用 Rust，介面用 React 與 TypeScript，Android 的平台功能用 Kotlin。目前只在 Windows 上建置過。

**需要的工具**

- Rust 1.93 以上，加上目標 `aarch64-linux-android`（實機）與 `x86_64-linux-android`（模擬器）
- Node.js 與 npm
- JDK、Android SDK 與 NDK；設定好 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`
- Windows 上要開啟「開發人員模式」（Tauri 建置的最後一步要建立符號連結）

**建置**

```powershell
cd app
npm ci
npm run build
npx tauri android build --apk --debug --target aarch64   # 模擬器用 x86_64
```

APK 在 `app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/`。版本庫放在純英文的路徑下，可以避開 Android 工具對路徑的限制。

正式版要用自己的簽章金鑰：把 `app/src-tauri/gen/android/keystore.properties.example` 複製成 `keystore.properties` 填好，再執行 `pwsh -NoProfile -File app/scripts/release-android.ps1`。它分架構建 arm64 與 x86_64、核對簽章，把安裝檔與 `SHA256SUMS.txt` 放到 `app/target/release-apk/`。沒有 `keystore.properties` 時建出的正式版是未簽章的，不能直接安裝。

**檢查**

```powershell
pwsh -NoProfile -File app/scripts/check.ps1
```

依序跑型別檢查、靜態檢查、介面測試、建置、rustfmt、clippy、cargo-deny 與全部測試（需要先安裝 cargo-deny）。程式的組織見 [`app/README.md`](app/README.md)。

## 隱私

- **基本功能不連網**：加入、閱讀、劃線、筆記、紀錄、匯出、備份、同步包、整理文獻庫與文獻之間的連結都在裝置上完成。
- **只有你啟用的功能會連網**：
  - 線上翻譯與 AI 重點筆記：把論文的文字送到你選的服務。每次送出前會說明送到哪裡、送什麼，你同意才送；可以在設定裡改成不再詢問。
  - 下載模型：離線翻譯的語言模型（約 34 MB）與版面模型（約 124 MB），都要你同意才下載。之後在裝置上執行，不送出論文內容。
  - 設定裡的「測試服務」：連到你填的翻譯服務，只送一句固定的短句。
- **不需要帳號，不收集使用資料**。翻譯服務的金鑰加密存在裝置上，用的是你自己的帳號與額度。
- 文獻、筆記與紀錄都是你裝置上的檔案，備份檔可以整包帶走。完整的說明見[隱私說明](docs/privacy.md)。

## 文件

除了隱私說明，以下文件目前只有繁體中文。

**使用者文件**（在 [`docs/`](docs/)）

| 文件 | 內容 |
|---|---|
| [安裝與第一次使用](docs/安裝與第一次使用.md) | 下載安裝、文獻庫放在哪裡（「所有檔案存取」的取捨）、加入第一篇 |
| [閱讀與劃記](docs/閱讀與劃記.md) | 工具箱、原文與譯本切換、原頁疊譯文、雙擊放大、劃線、短評、筆刷、跳頁、大綱、引用卡片與文獻之間的連結 |
| [筆記與 Obsidian](docs/筆記與Obsidian.md) | 筆記檔在哪、長什麼樣子、怎麼讓 Obsidian 讀寫同一份 |
| [翻譯與重點筆記](docs/翻譯與重點筆記.md) | 各服務的金鑰、費用、送出前詢問、離線翻譯與版面模型、提示詞與術語庫、重點筆記 |
| [備份與同步包](docs/備份與同步包.md) | 備份檔與同步包的不同、兩台怎麼搬、衝突怎麼選、已知限制 |
| [常見問題](docs/常見問題.md) | 連網、解除安裝、檢查發現的問題、翻譯失敗等 |
| [設計理念](docs/設計理念.md) | 資料以檔案為本、一台裝置就完整、可換的產生方法、外掛 |
| [文獻庫格式](docs/格式/文獻庫.md) | 資料夾、`paper.json`、材料、閱讀紀錄、筆記檔、`ink.json`、`references.json` 的欄位與相容規則（0.x 期間可能變） |
| [隱私說明](docs/privacy.md)（[English](docs/privacy.en.md)） | 資料存在哪裡、什麼時候連網、送出什麼 |

**程式文件**

| 文件 | 內容 |
|---|---|
| [`app/README.md`](app/README.md) | 程式的組織、檢查與工具版本 |

## 參與

歡迎在 GitHub 提出 issue，回報問題或提出建議。提交程式之前請先跑上一節的檢查；提交說明照 [Conventional Commits](https://www.conventionalcommits.org/) 的格式。

## 授權

本專案以 [Apache License 2.0](LICENSE) 授權。使用的第三方組件與各自的授權列在應用的「關於」頁。
