# Liberation Sans（PDF 標準字型 Helvetica 的替代字型）

PDF 沒有內嵌字型、只寫「Helvetica」「Arial」這類標準字型名稱時，pdf.js 用這四個字型檔來畫。pdf.js 套件內附的是 Liberation 1.07.4（GPL-2.0 加字型例外）；這裡換成 Liberation 官方發布的 2.1.5（SIL Open Font License 1.1），建置時由 `app/scripts/copy-pdfjs-assets.mjs` 蓋過 `public/pdfjs/standard_fonts/` 裡的同名檔，授權檔 `LICENSE_LIBERATION` 也換成這裡的 `LICENSE`。

## 來源

| 項目 | 內容 |
|---|---|
| 專案 | Liberation Fonts，https://github.com/liberationfonts/liberation-fonts |
| 版本 | 2.1.5 |
| 發布頁 | https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5（2021-09-30） |
| 壓縮檔 | https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz（發布說明裡附的連結；這個發布沒有另掛「Assets」檔） |
| 壓縮檔的 SHA-256 | `7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0` |
| 取得日期 | 2026-11-16 |
| 授權 | SIL Open Font License 1.1（`LICENSE`，含著作權聲明：Google 的 Arimo 數位資料與 Red Hat；保留字型名稱 Liberation、Arimo） |

字型檔內的名稱表也寫著「Version 2.1.5」與「Licensed under the SIL Open Font License, Version 1.1」。

## 內含的檔

壓縮檔裡只取 Liberation Sans 的四個樣式與授權、作者名單，原樣不改，檔名和 pdf.js 預期的相同。

| 檔案 | SHA-256 |
|---|---|
| `LiberationSans-Regular.ttf` | `76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8` |
| `LiberationSans-Bold.ttf` | `788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173` |
| `LiberationSans-Italic.ttf` | `e5bae5c4cde31f22142753855f4f8fb86da6ff39955ed3c0a11248b0d16948b0` |
| `LiberationSans-BoldItalic.ttf` | `698da70fc191cc5f33ad4d6d3fe830fe4624b898ea2e3169955928b7c491f1ee` |
| `LICENSE` | `93fed46019c38bbe566b479d22148e2e8a1e85ada614accb0211c37b2c61c19b` |
| `AUTHORS` | `b627404917ef824675cb24c32e2facd0b57650627955f3fc3d8a8863c5d0adfc` |

## 義務

OFL 1.1 允許和軟體一起散布，條件是每一份都附著作權聲明與授權全文（`LICENSE`），字型不單獨出售。改動字型就不能再用保留的名稱，所以這裡一律原樣使用；要換版本時整組換掉，更新這份說明的版本與雜湊（`app/scripts/licenses.mjs` 從這裡的「版本」讀版本號）。
