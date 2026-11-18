# app/translate：翻譯引擎

給一批段落，交回譯文。套件名稱 `paperreader-translate`。

這個套件會連網，所以不放進核心（核心不連網）。它不知道文獻庫與檔案格式：外殼（`app/src-tauri`）把要翻的段落交進來，再把譯文交給核心寫進譯文疊加檔。核心不依賴它，它也不依賴核心。

## 做什麼

| 引擎 | 怎麼接 | 檔案 |
|---|---|---|
| 對話模型：Anthropic、OpenAI、Gemini、OpenAI 相容端點（含 Ollama） | 現成的 genai 函式庫，HTTP 用戶端由我們自己建後交給它 | `src/llm.rs`、`src/prompt.rs` |
| 機器翻譯服務：DeepL、Google Cloud Translation v2 | 照官方 REST API | `src/mt.rs` |
| 裝置上的引擎：Firefox 翻譯（不連網） | 只定接口 `DeviceTranslator`，由外殼實作（實際在畫面一側的背景工作者跑） | `src/device.rs` |

另外：

- 工作迴圈（分批、重試、取消、每批的進度）：`src/job.rs`、`src/cancel.rs`。
- 引擎申報：每種引擎要填什麼、會不會把文字送出裝置、一批多大、支援哪些目標語言：`src/engines.rs`。畫面的翻譯服務清單依它畫。
- 譯文的自動檢查（看起來沒翻、數字或引用編號或公式或網址不見了、亂碼、過短）：`src/check.rs`。
- 術語庫（三欄 TSV 的讀寫、挑出一段原文用得到的術語）：`src/glossary.rs`。
- 選用模型檔的下載與核對（zstd 解壓、SHA-256；一百多 MB 的檔邊收邊寫進檔案）：`src/download.rs`。
- 版面模型（PP-DocLayoutV3）的執行，只用在翻譯的分段：`src/layout_model.rs`，`layout-model` 功能開了才編進 ONNX Runtime（`ort`，建置時從 cdn.pyke.io 取原生程式庫）。整個程序一個 ONNX Runtime 環境；外殼把畫面交來的頁面圖交進來，交回方框。電腦上量準確度用 `examples/layout_boxes.rs`。
- HTTPS：rustls 加 ring，根憑證內含（webpki-roots），不依賴系統的憑證庫：`src/http.rs`。
- 錯誤分種類（金鑰不對、額度、連不上、太頻繁、回應讀不懂…），畫面依種類說明：`src/error.rs`。

金鑰只放在 `EngineConfig` 裡傳進來，不寫進任何記錄或錯誤訊息。

## 從哪裡看起

先看 `src/lib.rs` 開頭的說明與公開的型別（`Provider`、`EngineConfig`、`Params`、`Segment`），再看 `src/job.rs` 的 `run_job`：外殼就是呼叫它。

## 和其他部分的關係

```
畫面（設定、產生畫面）──指令──▶ 外殼 src-tauri/src/translate.rs ──▶ 本套件 run_job ──▶ 各家服務
                                        │                                   ▲
                                        └──▶ 核心：讀段落、寫譯文疊加檔        │
                                 外殼 src-tauri/src/ondevice.rs ──實作 DeviceTranslator──┘（Firefox 翻譯在畫面一側跑）
```

- 外殼管設定（服務清單、主要與備用）、金鑰（經平台轉接器存在 Android Keystore）、背景工作與進度，把每批結果寫進核心。
- 用到的外部函式庫與授權列在應用的「關於」頁。

## 測試

在 `app/` 底下：

```powershell
cargo test -p paperreader-translate
```

`tests/engines.rs` 對每家引擎起一個本機的假伺服器（`tests/common/`），驗請求長什麼樣、回應怎麼讀、錯誤怎麼分、重試與取消；`tests/device.rs` 驗裝置上的引擎經同一條工作迴圈（分批、自動檢查、出錯停下）；`tests/download.rs` 驗大檔下載失敗或取消不留檔。不連外網、不需要金鑰，也不需要版面模型的模型檔（真的跑模型的測試要設 `PR_LAYOUT_MODEL` 與 `PR_LAYOUT_RGB` 才跑）。

只測翻譯套件、不編 ONNX Runtime：`cargo test -p paperreader-translate`（不開 `layout-model`）。整個工作區（`cargo test --workspace`）會跟著外殼的預設開 `layout-model`。
