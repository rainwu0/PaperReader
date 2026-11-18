# 版面模型：模型檔與前後處理

翻譯的分段可以選用版面模型 PP-DocLayoutV3。這份說明模型檔從哪裡來、電腦上量測時怎麼拿到它，以及應用的前處理、後處理照的是什麼。做法經過三個模型的比較與接進應用前的試作；結論都已寫進下面列的程式。

## 模型檔

| 項目 | 內容 |
|---|---|
| 模型 | PP-DocLayoutV3 的官方 ONNX 版，PaddlePaddle 在 Hugging Face 的頁面：https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_onnx |
| 版本 | 釘在一個 commit，網址與 SHA-256 在 `app/src-tauri/src/commands/layout.rs` 的 `MODEL` |
| 授權 | Apache-2.0（模型卡） |
| 應用裡 | 不內含。使用者在設定 → 產生同意後下載到應用私有的空間，核對雜湊；可以刪 |
| 電腦上量測 | 自己從上面的網址下載 `inference.onnx`（不進版本庫），用 `translate/examples/layout_boxes.rs` 跑 |

## 前處理與後處理

| 步驟 | 做法 | 程式 |
|---|---|---|
| 畫頁面 | 長邊 1600 像素畫一頁，整張縮成 800×800（不保持長寬比），取 RGB | `src/translate/pageRegions.ts` |
| 推論 | 輸入除以 255、不減平均值；另給 `im_shape`（輸入的高、寬）與 `scale_factor`（輸入對原圖的縮放）；輸出每列是類別、分數、方框（原圖座標） | `translate/src/layout_model.rs` |
| 類別 | 25 類，依模型輸出的編號排列 | `translate/src/layout_model.rs` 的 `LABELS` |
| 類別對到角色 | 要翻、不翻、略過 | `src/translate/regions.ts` 的 `ROLES` |
| 分段 | 一個區塊一段；區塊要翻而且規則也沒丟掉的字才翻；類別無關的 NMS（IoU 0.7） | `src/translate/regions.ts` |
