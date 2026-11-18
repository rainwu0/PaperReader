# app/scripts：檢查與驗收的腳本

| 檔案 | 做什麼 |
|---|---|
| `check.ps1` | 提交前、日後持續整合要跑的整組檢查：介面型別檢查、介面靜態檢查、介面邏輯測試、外掛的邊界、介面相依的漏洞（`npm audit`）與授權、介面建置、排版、靜態檢查、Rust 相依的授權與漏洞、測試 |
| `ext-boundary.mjs` | 外掛的邊界：登記處（`src/ext/index.ts`、`src-tauri/src/ext.rs`、兩個 `Cargo` 檔）以外沒有檔案引用外掛；外掛名稱取 `ext/` 與 `src/ext/` 的子資料夾，不寫死。`check.ps1` 的一步；`ext-boundary-test.mjs` 是它的測試。外掛自己的測試放在外掛的資料夾（`src/ext/<名稱>/*-test.mjs`、`ext/<名稱>/tests/`），`npm run test:ui` 用萬用字元收進來 |
| `npm-licenses.mjs` | 介面相依（npm）的授權檢查：讀 `node_modules` 裡每個套件的授權欄位，對照和 `app/deny.toml` 相同的允許清單（另加 tslib 的 0BSD）。`check.ps1` 的一步 |
| `notes-test.mjs` | 介面邏輯測試：筆記窗格撞上別處的修改時，算出哪一段沒存進去（`src/screens/reader/notesConflict.ts`）。`npm run test:ui` 的一部分 |
| `dev-env.ps1` | 點號載入（`. app\scripts\dev-env.ps1`）：在目前的視窗設定 Android 建置要用的 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`。已經設了就照用；沒設時從 `PAPERREADER_TOOLS`（放 `jdk17` 與 `Android\Sdk` 的資料夾）推出來；都沒有就停下說明 |
| `emulator.ps1` | 建立並啟動一台模擬器（`-Name`，預設 `pilot`；`-Port` 指定序號 `emulator-<Port>`），等它開機 |
| `fake-github-server.mjs` | 驗收應用內更新用的假 GitHub：Releases API 加附件下載（轉到另一個埠，和 GitHub 一樣），附件的核對碼由這裡算；`--mode corrupt` 下載時改壞安裝檔、`--mode ratelimit` 清單回 403。除錯版在編譯時設 `PAPERREADER_UPDATE_BASE=http://10.0.2.2:<埠>` 才會問它；正式版寫死 GitHub |
| `make-test-pdfs.py` | 產生驗收用的三份合成 PDF（30 頁、約 5 MB、620 頁約 53 MB）；`walkthrough.ps1` 與 `emulator-check.ps1` 在 `app/target/test-pdfs/` 沒有時自己呼叫 |
| `emulator-check.ps1` | 在 Android 模擬器上跑核心（文獻庫）的驗收項目（十九項），用命令列工具 `prcli` |
| `walkthrough.ps1` | 在模擬器上走「怎麼算成功」七件事各一條主流程並截圖：收進來、接著讀、讀過什麼、找到某一篇、記下想法、不失去東西、對照母語。`-Serial` 必填（指定哪一台模擬器）；`-Only 5` 只跑第 5 條；輸出預設在 `app/target/walkthrough/`（`-OutDir` 可改）。細節在 `walkthrough/` |
| `copy-pdfjs-assets.mjs` | 把 PDF.js 執行時要讀的資源（字型、字元對照表、wasm、色彩描述檔）從 `node_modules` 複製到 `public/pdfjs/`；介面建置前自動執行 |
| `licenses.mjs` | 「關於」頁的組件與授權清單：從 `cargo metadata`（`Cargo.lock`）、`package-lock.json`、`build.gradle.kts` 與內含的檔產生 `public/licenses.json`，附各組件的授權全文（沒附的用 `licenses/` 的標準全文）；介面建置前自動執行。`licenses-test.mjs` 是它的測試（`npm run test:ui` 的一部分） |
| `catalog-test.mjs`、`pdfmeta-test.mjs` | 介面邏輯測試（搜尋、排序、篩選、檢查問題的說法；PDF 自帶篇名與作者的取捨），用 Node 內建的測試器：`npm run test:ui` |
| `walkthrough/` | 驗收腳本的內容：`run.mjs`（主程式：準備模擬器、推測試檔、依序跑七條）、`flows.mjs`（七條主流程）、`strings.mjs`（從應用的字串表依字串鍵取字）、`lib.mjs`（adb 與真的觸控、系統畫面的自動操作、經 Chrome 開發者協定讀應用裡的網頁、讀截圖的像素）、`make-pdfs.mjs`（產生有大綱、有自帶篇名與作者的測試 PDF） |

## 跑驗收

```powershell
# 模擬器要先開機；adb devices 看它的序號
pwsh -NoProfile -File app\scripts\walkthrough.ps1 -Serial emulator-5554             # 建置、安裝、七條主流程、截圖
pwsh -NoProfile -File app\scripts\walkthrough.ps1 -Serial emulator-5554 -SkipBuild  # 用已經建好的 APK
```

寫法：點按一律是真的觸控（`adb shell input`）；找應用畫面上的元件用的文字一律用字串鍵從字串表取（`T("home.empty.action")`），不寫死畫面上的字，字串鍵不見了會直接報錯；系統的畫面（授權頁、選檔、存檔）是 Android 的英文介面，用它們的文字找。每條結束記一筆通過或不通過與看到的內容，存在輸出資料夾的 `results.json`。

測試用的 PDF 是合成的（`make-test-pdfs.py` 產生到 `app/target/test-pdfs/`，與 `walkthrough/make-pdfs.mjs`，不進版本庫；在別的 worktree 裡時直接用主工作資料夾已經產生的那一份）。驗收時模擬器的螢幕會被設成保持亮著。所有檔案存取的權限由腳本在系統的授權頁按開關；實機上要使用者自己按。
