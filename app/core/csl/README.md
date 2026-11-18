# CSL 格式檔與語系檔

這個資料夾裡的 `.csl` 與 `.xml` 來自 CSL（Citation Style Language）專案，<https://citationstyles.org/>。原樣內含，沒有修改；應用在匯出引用格式時讀它們（`src/citation.rs`）。

## 授權

這些檔案以 [Creative Commons Attribution-ShareAlike 3.0 Unported](https://creativecommons.org/licenses/by-sa/3.0/)（CC BY-SA 3.0）授權，不屬於本專案的 Apache-2.0 授權範圍。散布時要保留每個檔案 `<info>` 裡的作者、貢獻者、譯者名單（下表照抄）。修改這些檔案的話，修改後的檔案同樣要以 CC BY-SA 3.0 發布。

## 來源

| 檔案 | 內容 | 來源版本庫與提交 | 作者與貢獻者（照檔案的 `<info>`） |
|---|---|---|---|
| `apa.csl` | APA Style 7th edition | citation-style-language/styles @ `d16ba6427ca3c5dada6578a6f7a0f98bd3ea03b3` | 作者 Brenton M. Wiernik、Andrew Dunning；貢獻者 Jay Pfaffman |
| `ieee.csl` | IEEE Reference Guide version 11.29.2023 | 同上 | 作者 Michael Berkowitz；貢獻者 Julian Onions、Rintze Zelle、Stephen Frank、Sebastian Karcher、Giuseppe Silano、Patrick O'Brien、Brenton M. Wiernik、Oliver Couch、Andrew Dunning |
| `chicago-author-date.csl` | Chicago Manual of Style 18th edition (author-date) | 同上 | 作者 Andrew Dunning |
| `modern-language-association.csl` | MLA Handbook 9th edition | 同上 | 作者 Sebastian Karcher、Andrew Dunning；貢獻者 Patrick O'Brien |
| `locales-en-US.xml` | 英文（美國）語系 | citation-style-language/locales @ `14940e56f7c783e4173cafbd92e0f5856f2caa14` | 譯者 Andrew Dunning、Sebastian Karcher、Rintze M. Zelle、Denis Meier、Brenton M. Wiernik |
| `locales-zh-TW.xml` | 繁體中文（臺灣）語系 | 同上 | 譯者 sati-bodhi |

取得日期：2026-11-15，從 `https://raw.githubusercontent.com/citation-style-language/{styles,locales}/<提交>/<檔名>` 下載。

## 更新

換新版時從同兩個官方版本庫取檔、原樣放進來，更新上表的提交與名單，再跑 `cargo test -p paperreader-core citation`。
