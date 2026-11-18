# 參與貢獻

謝謝你有興趣。這份文件說明怎麼建置、提交之前要做什麼、以及貢獻的規則。English summary at the end.

## 回報問題與提出建議

到 GitHub 的 Issues 用範本開一則。安全問題請照 [`SECURITY.md`](SECURITY.md)，不要開公開的 issue。

## 建置

應用以 Tauri 2 建成：核心與外殼用 Rust，介面用 React 與 TypeScript，Android 的平台功能用 Kotlin。完整的工具清單與 Android 建置步驟在 [`README.md`](README.md)「從原始碼建置」；程式的組織在 [`app/README.md`](app/README.md)。

在 `app/` 資料夾裡常用的指令（定義在 `app/package.json`）：

| 指令 | 做什麼 |
|---|---|
| `npm ci` | 依 `package-lock.json` 安裝介面的相依 |
| `npm run dev` | 啟動介面的開發伺服器 |
| `npm run typecheck` | 型別檢查 |
| `npm run lint` | 靜態檢查（Oxlint） |
| `npm run test:ui` | 介面邏輯測試 |
| `npm run build` | 建置介面（Rust 的外殼編譯時要內嵌建置結果） |
| `cargo test --workspace` | Rust 的全部測試 |

## 提交之前

在版本庫根目錄執行：

```powershell
pwsh -NoProfile -File app/scripts/check.ps1
```

它依序跑型別檢查、靜態檢查、介面測試、介面建置、`cargo fmt --check`、`cargo clippy -D warnings`、`cargo deny check` 與 `cargo test --workspace`。需要先安裝 [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)。全部通過再送出拉取要求；GitHub 上也會跑同一組檢查。

## 提交說明

照 [Conventional Commits](https://www.conventionalcommits.org/) 的格式：`類型: 說明`。類型用 `feat`、`fix`、`docs`、`test`、`refactor`、`chore`。說明可以用中文或英文，一個提交做一件事。

## 程式碼與文件的語言

程式註解與專案文件以繁體中文為主，因為維護者與目標使用者以繁體中文為主。對外的 README、隱私說明、本文件另附英文。你用英文寫的 issue 與拉取要求都歡迎。

## 版號與發布

版號的寫法、各段的意義與 Android 的 versionCode 見[版號規則](規格/05_版號規則.md)。對外的版號用點（`0.1.12`、修正版 `0.1.12.3`）；用 `node app/scripts/bump-version.mjs <新版號>` 一次改好各處（設定檔裡自動換成工具收得下的 `0.1.12+3`）；`app/scripts/check.ps1` 會檢查三處版號與 versionCode 一致；發布 Android 安裝檔用 `app/scripts/release-android.ps1`。

## 授權

本專案以 [Apache License 2.0](LICENSE) 授權。你送出的貢獻，視為依同一份授權提供給本專案。

## 參考別人的專案

可以學別人的做法，不可以抄：

- 可以讀文件、觀察產品怎麼運作、採用公開的標準與資料格式、依授權使用函式庫。
- 不要複製別人的程式碼、樣式、圖示與介面文字，不要照著別人的畫面排版重畫，不要使用別人的名稱或商標。
- 要看別人的原始碼才弄得懂時，讀懂後關掉，用自己的設計重寫，並在註解或說明裡寫明參考了哪個專案的哪個做法。
- 新增外部函式庫時，確認授權與 Apache-2.0 相容（`cargo deny check` 只放行寬鬆的授權）。要求衍生作品同樣開源的授權（GPL、AGPL 等）請先在 issue 討論。

## English summary

- **Build:** Tauri 2 (Rust core and shell, React and TypeScript UI, Kotlin for Android). See `README.md` ("Build from Source") and `app/README.md`. Useful commands in `app/`: `npm ci`, `npm run dev`, `npm run typecheck`, `npm run lint`, `npm run test:ui`, `npm run build`, `cargo test --workspace`.
- **Before submitting:** run `pwsh -NoProfile -File app/scripts/check.ps1` (type check, lint, UI tests, build, rustfmt, clippy with warnings denied, cargo-deny, Rust tests). CI runs the same checks.
- **Commits:** Conventional Commits (`feat`, `fix`, `docs`, `test`, `refactor`, `chore`); one change per commit.
- **Versions:** the version format and what each part means are in `規格/05_版號規則.md` (Traditional Chinese). `node app/scripts/bump-version.mjs <version>` updates every place at once; `check.ps1` verifies they agree.
- **Language:** code comments and project docs are mainly in Traditional Chinese; issues and pull requests in English are welcome.
- **License:** contributions are licensed under Apache-2.0, the same as the project.
- **Learning from other projects:** learn from their approach, do not copy code, styles, icons, UI text, layouts, names or trademarks. If you had to read source to understand something, rewrite it from your own design and note the reference. Make sure any new third-party library has a permissive license.
