# app：PaperReader 的程式

這個資料夾放 PaperReader 的程式：核心（Rust）、應用外殼（Tauri 2）、介面（React 加 TypeScript）、自有的 Android 外掛（Kotlin）。核心有文獻庫格式與裝置本機的文獻庫接口、閱讀紀錄與回收區，以及群組、筆記檔、筆跡、譯文疊加檔、備份與還原、書目檔輸出；應用是 Android 版全局功能的最少版本。

## 子資料夾

| 路徑 | 內容 |
|---|---|
| `core/` | `paperreader-core`：讀寫文獻庫資料夾的核心函式庫，不依賴 Android 與 Tauri，可以在電腦上直接測試。說明見 `core/README.md` |
| `src/` | 介面：React 加 TypeScript。顯示與操作，不含資料規則。說明見 `src/README.md` |
| `src-tauri/` | 應用的組裝與指令：Tauri 2 外殼，把介面與核心接起來，只轉接。說明見 `src-tauri/README.md` |
| `translate/` | 翻譯引擎：對話模型與機器翻譯服務的轉接、分批、重試、取消。由外殼呼叫，不依賴核心，核心也不依賴它 |
| `plugins/` | 平台轉接器（Android 用 Kotlin，以 Tauri 外掛的形式寫成）：所有檔案存取的權限與儲存位置、系統選檔（可多份）、`paperreader://` 連結、存到、分享、列印、用其他應用打開、金鑰的保管（Android Keystore）、螢幕邊界與系統列明暗、閱讀時螢幕不關。說明見 `plugins/README.md` |
| `cli/` | `prcli`：包住核心函式庫的小型命令列工具，只用來在模擬器上驗證，不是產品的一部分 |
| `scripts/` | `check.ps1`：提交前與持續整合跑的整組檢查（八步）；`emulator-check.ps1`：在 Android 模擬器上跑文獻庫的驗收項目（核心的十九項）；`walkthrough.ps1`：在模擬器上走一遍應用的流程並截圖。說明見 `scripts/README.md` |
| `target/` | 編譯產物，不進版本庫 |

## 從哪裡看起

1. 格式：[`docs/格式/文獻庫.md`](../docs/格式/文獻庫.md)（資料夾與檔案的格式）。
2. 程式：從 `core/README.md` 開始，它列出各模組負責什麼。

## 怎麼跑檢查

在 PowerShell 執行：

```powershell
pwsh -NoProfile -File app\scripts\check.ps1
```

依序跑八步，任何一步失敗就停下：

1. `npm run typecheck`：介面的型別檢查（`tsc --noEmit`，嚴格模式）。
2. `npm run lint`：介面的靜態檢查（Oxlint）。
3. `npm run test:ui`：介面邏輯的測試。
4. `npm run build`：介面建置。Rust 的應用外殼編譯時要內嵌建置結果，沒有它外殼編不過。
5. `cargo fmt --all --check`：排版符合 rustfmt。
6. `cargo clippy --workspace --all-targets -- -D warnings`：靜態檢查，警告即失敗。
7. `cargo deny check`：相依的授權、已知漏洞、來源。
8. `cargo test --workspace`：全部測試。

模擬器上的驗收另外執行 `pwsh -NoProfile -File app\scripts\emulator-check.ps1`（需要 JDK、Android SDK、NDK 與模擬器，位置的設法見 `scripts/dev-env.ps1`，用法見腳本開頭的說明）。

## 工具版本

目前用的是 Rust 1.93.1（Rust 版次 2024）。`Cargo.toml` 的 `rust-version = "1.93"` 規定最低版本，工具鏈太舊時 cargo 會直接報錯。需要的元件與目標：

- 元件：`rustfmt`、`clippy`。
- 目標：`x86_64-linux-android`（模擬器）、`aarch64-linux-android`（實機）。

這裡沒有用 `rust-toolchain.toml` 鎖定確切版本；公開前接上持續整合時，再用它鎖定確切的版本、元件與目標。

## 設定檔

| 檔案 | 作用 |
|---|---|
| `Cargo.toml` | 工作區：成員、共用的版本、版次與最低 Rust 版本、檢查等級（clippy 預設等級，警告即失敗） |
| `rustfmt.toml` | 排版：只寫明樣式版次，其餘用 rustfmt 的預設值 |
| `deny.toml` | 相依檢查：允許的授權、已知漏洞、來源 |
| `Cargo.lock` | 相依的確切版本 |

相依套件的名稱、版本、授權與用途列在應用的「關於」頁。介面的相依在 `package.json` 與 `package-lock.json`（`npm ci` 依鎖定的版本安裝）；`.oxlintrc.json` 是 Oxlint 的設定，`tsconfig.json` 是型別檢查的設定。
