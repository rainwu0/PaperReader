# app/cli：命令列工具 prcli

第二個驅動核心（`app/core`）的程式，用來在 Android 模擬器上直接驗核心，不經過應用的畫面。套件名稱 `prcli`。

它是除了應用之外唯一呼叫核心的程式。有它，就能確認核心的規則不靠畫面也成立，也能在模擬器真正的檔案系統上驗寫入、改名、權限這些只有在裝置上才看得到的行為。它不是給使用者用的。

## 怎麼用

```
prcli <文獻庫的根目錄> <裝置代號檔> <指令> [參數]
```

- 每次執行都重新開啟文獻庫，做一件事就結束。
- 結果以一行 JSON 寫到標準輸出；失敗時寫 `{"error":"<錯誤種類>","detail":"…"}`，結束碼 1。
- 裝置代號檔裡放這台裝置的代號（沒有就建一個）。

指令依主題：

| 主題 | 指令 |
|---|---|
| 文獻庫與單篇 | `init`、`add`、`list`、`get`、`set-title`、`remove`、`restore`、`trash-list`、`remove-material`、`trash-materials`、`restore-material`、`purge`、`empty-trash`、`check`、`rescan-after`、`set-pages` |
| 閱讀紀錄 | `open-paper`、`position`、`set-read`、`progress`、`continue`、`events` |
| 群組 | `groups`、`group-create`、`group-rename`、`group-delete`、`group-add` |
| 譯本 | `add-translation` |
| 筆記與劃線、筆跡 | `notes`、`highlight-add`、`highlight-comment`、`highlight-delete`、`thoughts`、`ink`、`stroke-add`、`stroke-delete`、`ink-clear-page` |
| 備份 | `export-backup`、`restore-backup` |
| 同步包 | `changes`、`sync-export`、`sync-import`、`sync-resume`、`sync-join`、`sync-folder-list`、`sync-folder-put`、`sync-folder-fetch` |
| 整批匯入 | `import`（資料夾或 zip；`--dry-run` 只印預覽） |
| 測試用 | `stress-edit`（連續改寫，驗原子寫入） |

每個指令的參數見 `src/main.rs` 的 `run`。

## 從哪裡看起

`src/main.rs`：`parse` 讀參數，`run` 把指令一對一轉給核心 `LocalLibrary` 的公開方法。這裡不加任何資料規則，規則都在核心。

## 和其他部分的關係

- 依賴：只有核心。
- 用的人：`app/scripts/emulator-check.ps1` 把它編成 Android 執行檔（`cargo build --release --target x86_64-linux-android -p prcli`），推到模擬器的 `/data/local/tmp/`，依序跑核心的驗收項目。
- 測試：`tests/cli.rs`、`tests/device_file.rs` 在電腦上直接執行編好的 `prcli`，驗輸出的 JSON 與結束碼。在 `app/` 底下 `cargo test -p prcli`。
