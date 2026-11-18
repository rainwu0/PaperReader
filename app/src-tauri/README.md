# app/src-tauri：應用的組裝與指令

Tauri 2 的外殼。把網頁做的介面與核心（`app/core`）接起來。**只轉接，不含資料規則**。

## 從哪裡看起

| 路徑 | 內容 |
|---|---|
| `src/lib.rs` | 組裝：註冊外掛與指令，建立狀態，啟動時清掉上次中斷留下的選檔暫存 |
| `src/commands/` | 指令，依主題分檔：`library`（文獻庫的狀況、建立、掃描、檢查）、`papers`（目錄與單篇）、`add`（加入）、`reading`（閱讀事件與分段讀原文）、`app`（版本、深淺色、螢幕邊界）、`backup`（備份檔、從備份檔還原、從另一台加入）、`sync`（同步包的匯出、匯入的準備與寫入、重試沒做完的合併、進度） |
| `src/transport.rs` | 手動選檔的傳輸：核心 `SyncTransport` 的外殼實作（取使用者選好的那一份、放進交出去的暫存資料夾） |
| `src/state.rs` | 應用的狀態：設定檔（裝置代號、文獻庫放哪、深淺色）與開著的文獻庫。文獻庫與設定各一把鎖（先拿文獻庫的、再拿設定的），深淺色不必等文獻庫；耗時的指令放到背景執行緒做。設定檔讀不懂時不讓啟動失敗：原檔改名留著、能讀出的裝置代號保留、其餘用預設值，畫面告知一次；先存設定、成功了才改狀態。記得的位置沒有文獻庫時回報「不見了」，不自己建新的。從備份檔還原與從另一台加入：目前的文獻庫是空的（或還沒有）時放在正式位置（先還原到旁邊、成功後換位），不是空的放在旁邊的新資料夾（`place_archive`） |
| `src/location.rs` | 文獻庫放在哪裡的決定（純函式，有測試）：還沒建立、開啟、失去權限、原位置不見了 |
| `src/error.rs` | 指令的錯誤：「種類」加「細節」。每一種核心的錯誤都有對應（編譯器與測試把關）；細節不含篇名、路徑 |
| `capabilities/` | 網頁被允許呼叫什麼：只開用到的。介面只呼叫應用自己的指令，所以一項都沒開 |
| `gen/android/` | Tauri 產生的 Android 專案，已改過 `AndroidManifest.xml`（權限、關掉系統備份、`paperreader://` 連結、接收別的應用分享的 PDF）與 `res/xml/file_paths.xml`（FileProvider 的範圍縮小）。可以提交，建置輸出已在 `.gitignore` |

「關於」頁列的組件與授權不在這裡：介面建置前由 `app/scripts/licenses.mjs` 從 `Cargo.lock` 與 `package-lock.json` 產生 `licenses.json`，畫面直接讀。
| `icons/` | 應用圖示 |

## 指令與畫面

畫面只經過 `app/src/api` 呼叫這裡的指令。指令一對一轉給核心的操作，交回核心的資料。例外是兩個只有外殼才知道的事：文獻庫放在哪個位置（共用空間或應用專屬空間），與選到的檔案是不是 PDF。

## 建置與檢查

在 `app/` 底下：

```powershell
cargo test -p paperreader-app          # 這一層的測試（錯誤種類、位置決定、狀態）
. scripts\dev-env.ps1                  # 設定 JAVA_HOME、ANDROID_HOME、NDK_HOME（只影響目前的視窗）
npx tauri android build --apk --debug --target x86_64    # 模擬器用的除錯 APK
```

輸出在 `gen/android/app/build/outputs/apk/universal/debug/`。電腦上編譯需要先有 `app/dist/`（`npm run build`），因為外殼把介面內嵌進去。
