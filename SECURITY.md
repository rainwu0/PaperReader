# 安全政策

## 回報安全問題

請**不要**把安全問題開成公開的 issue。請用 GitHub 的私下回報功能：

1. 到本專案的 **Security** 分頁。
2. 選 **Report a vulnerability**（Private vulnerability reporting）。
3. 寫清楚怎麼重現、影響什麼、你用的版本與裝置。

這是回報安全問題的唯一管道，本專案不收電子郵件。回報只有維護者看得到。收到後會盡快確認，修好之後在更新紀錄（`CHANGELOG.md`）裡說明。

## 支援的版本

| 版本 | 是否收到安全修補 |
|---|---|
| 最新的 0.x 版 | 是 |
| 較舊的 0.x 版 | 否，請更新到最新版 |

0.x 期間只修最新的一版。

## 範圍

屬於範圍內：

- PaperReader 應用本身的程式（核心、外殼、介面、Android 外掛）。
- 應用讀寫文獻庫、備份檔與設定檔的方式，例如檔案被寫到不該寫的位置、金鑰外洩。

不在範圍內：

- 使用者自己選的翻譯服務（Claude、OpenAI、Gemini、DeepL、Google Cloud Translation 或自架的端點）本身的問題。請向該服務的提供者回報。
- 使用者自己填寫的服務網址所指向的伺服器。
- 需要先取得使用者裝置的實體控制權或 root 權限才能做到的攻擊。
- 第三方函式庫本身的弱點：請向該函式庫回報；如果本專案因此受影響，歡迎同時告知我們。

## English

**Reporting.** Please do not open a public issue for a security problem. Use GitHub's private reporting: open the repository's **Security** tab and choose **Report a vulnerability** (Private vulnerability reporting). This is the only channel; no email address is provided. Include steps to reproduce, the impact, and your version and device.

**Supported versions.** During 0.x only the latest release receives security fixes.

**Scope.** In scope: the PaperReader app itself (core, shell, UI, Android plugin) and how it reads and writes the library, backup files and settings. Out of scope: the translation services you choose to use (report those to their providers), servers behind service addresses you enter yourself, attacks that require physical control or root access to your device, and vulnerabilities in third-party libraries (report them upstream; let us know too if PaperReader is affected).
