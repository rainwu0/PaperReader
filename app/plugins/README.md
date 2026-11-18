# app/plugins：自有的 Android 外掛

Android 才有的事寫在這裡，用 Kotlin，不用 GPL 授權的社群外掛。

| 外掛 | 做什麼 |
|---|---|
| `tauri-plugin-paperreader/` | 查「所有檔案存取」權限與各個儲存位置、帶使用者到授權頁、開系統的選檔畫面（可以一次選多份）並把選到的每一份 PDF 複製成暫存檔、回報螢幕邊界、設定系統列圖示的明暗、閱讀時讓螢幕不自動關閉（視窗旗標，不需要權限） |

## 結構（照 Tauri 官方的外掛骨架）

| 路徑 | 內容 |
|---|---|
| `src/lib.rs` | Rust 這一側：註冊 Kotlin 外掛，提供 `PaperReaderExt`，讓應用的指令呼叫外掛。非 Android 的平台每個方法交回「不支援」 |
| `android/src/main/java/io/github/rainwu0/paperreader/plugin/PaperReaderPlugin.kt` | Kotlin 這一側 |
| `build.rs`、`permissions/` | 外掛沒有開放給網頁的指令，所以清單是空的；網頁只呼叫應用自己的指令 |

## 規則

- 外掛不含文獻庫的規則；它只回報平台的狀況、做平台才做得到的事。
- 耗時的檔案複製在背景執行緒，不佔主執行緒。
- 錯誤訊息不含檔名與路徑。一份檔案複製失敗不影響其他份，該份帶 `error`（`no_space` 或 `failed`）回報，並刪掉那一份的暫存資料夾；空間不足，應用把它對應成「空間不足」。
- 應用專屬空間的位置取不到外部空間時回報錯誤，不改用內部空間（改了位置，先前加入的文獻就看不到）。
- Kotlin 的排版工具（ktfmt）需要另外下載安裝，還沒裝；程式照 Android 官方的 Kotlin 風格指南寫。
