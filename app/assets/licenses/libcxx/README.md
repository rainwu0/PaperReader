# libc++ 與 libc++abi 的授權

`LICENSE.TXT` 是 Android NDK 附的授權聲明的原文摘錄，給「關於」頁用（`scripts/licenses.mjs`）。

| 項目 | 內容 |
|---|---|
| 來源 | Android NDK 28.2.13676358 的 `toolchains/llvm/prebuilt/windows-x86_64/NOTICE`（SHA-256 `f96f763beb66a7ba7a667647fc64c0226ace875e590c831fdd9579ec1c1d91e1`） |
| 摘錄的範圍 | 第 593 到 1216 行：libc++ 的授權（Apache-2.0 加 LLVM 例外，以及舊的 University of Illinois／MIT 雙授權），接著是 libc++abi 的同一份 |
| 為什麼要 | 版面模型的 ONNX Runtime 在 Android 上要 `libc++_shared.so`（含 libc++abi），安裝檔原樣內含（剝掉除錯資訊）；照 Apache-2.0 第 4 條附授權全文 |

原文沒有改過；換 NDK 版本時照同一個範圍重新摘錄。
