// 介面文字（整批匯入）。用詞：匯入清單、加入、略過、原文、譯本。
// 寫法：版面只放結論；略過的原因與細節收在可展開的清單裡。
// 原因代碼與提醒照核心交回的（`import.reason.<代碼>`、`import.notice.<種類>`）；不認得的用通用的一句（`add.error.title`）。
// 同一句話已經有鍵的直接用那個鍵：選單的標題 `home.add`、「重新匯入」`sync.apply.again`。
export const importStrings = {
  // 功能的名稱：首頁「＋」的選單、選資料夾或 zip 的面板、預覽頁都用這一個。選單的標題用 `home.add`
  "import.title": "整批匯入",

  // 首頁「＋」的選單
  "add.menu.pdf": "選 PDF",
  "add.menu.import.hint": "照匯入清單一次加入多篇",

  // 選資料夾或 zip
  "import.source.lead": "根目錄要有匯入清單 paperreader-import.json。",
  "import.source.folder": "選資料夾",
  "import.source.zip": "選 zip 檔",
  "import.source.details":
    "匯入清單寫明每篇的書目，以及哪個檔是原文、譯本、重點筆記。格式見專案的說明文件「匯入清單」。資料夾要在手機內部儲存空間；記憶卡或雲端的請先壓成 zip。",

  // 進行中
  "import.scan.busy": "正在讀取…",
  "import.scan.percent": "正在讀取… {pct}%",
  "import.step.busy": "正在匯入 {done}／{total} 篇",

  // 出錯
  "import.failed.title": "沒有匯入",
  "import.unsupported": "這個位置的資料夾讀不到，請改選裝置本身儲存空間裡的資料夾，或改用 zip。",
  "import.error.Unreadable": "找不到匯入清單，或清單的格式不對。",
  "import.error.Unreadable.detail":
    "清單要放在資料夾或 zip 的根目錄（不是包在一層資料夾裡），名稱是 paperreader-import.json。",
  "import.error.FormatTooNew": "這份清單要用新版的應用匯入。",
  "import.error.PermissionDenied": "要先允許存取所有檔案，或改用 zip。",
  // zip 的路徑用了反斜線：錯誤種類 `ImportZipBackslash`，照 `error.<種類>` 的寫法
  "error.ImportZipBackslash": "zip 的路徑用了反斜線，請重新壓縮。",
  "error.ImportZipBackslash.detail":
    "Windows 內建的 PowerShell 5.1 會這樣壓縮，zip 格式不允許。請改用 PowerShell 7 或其他壓縮工具重新壓縮。",
  "import.expired": "匯入的內容變了，請重新匯入。",

  // 預覽
  "import.preview.summary": "會加入 {accept} 篇，略過 {skip} 篇",
  "import.preview.size": "共 {size}",
  "import.preview.accept": "會加入（{n}）",
  "import.preview.skip": "略過（{n}）",
  "import.preview.line": "第 {n} 篇：{text}",
  "import.preview.nothing": "沒有要加入的文獻。",
  "import.preview.go": "匯入",

  // 略過的原因（核心的代碼）
  "import.reason.bad-entry": "清單的欄位不對：{detail}",
  "import.reason.too-many-files": "檔案超過 32 個",
  "import.reason.bad-role": "角色和形式不合：{detail}",
  "import.reason.no-source": "沒有原文",
  "import.reason.multiple-sources": "原文不只一個",
  "import.reason.bad-path": "路徑不合規則：{detail}",
  "import.reason.duplicate-path": "同一個檔列了兩次：{detail}",
  "import.reason.missing-file": "找不到檔案：{detail}",
  "import.reason.form-mismatch": "檔案內容不對：{detail}",
  "import.reason.checksum-mismatch": "檔案和清單的指紋不符：{detail}",
  "import.reason.already-present": "文獻庫裡已經有了：{detail}",
  "import.reason.in-trash": "在回收區裡",
  "import.reason.duplicate-source": "和前面一篇是同一份原文",
  "import.reason.failed": "寫入時出錯",

  // 提醒
  "import.notice.same-doi": "DOI 和「{detail}」相同",
  "import.notice.bib-fields-dropped": "書目有欄位沒收：{detail}",

  // 結果
  "import.result.title": "整批匯入完成",
  "import.result.added": "加入了 {n} 篇。",
  "import.result.skipped": "略過 {n} 篇",
  "import.result.failed": "{n} 篇寫入時出錯",
  "import.result.notDone": "{n} 篇沒有匯入，再匯入一次會接著做。",

  // 文獻資訊的材料
  "info.material.markdown": "譯本（Markdown）",
  "info.material.text": "抽出文字",
} as const;
