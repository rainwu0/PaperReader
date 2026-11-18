// 介面文字（產生、備份區）。用詞：產生、譯本、備份檔、從備份檔還原、文獻庫。
// 出錯的訊息用 `errorMessage(kind)`，原因用 `errorDetail(kind)` 放「詳情」；連網的功能留在畫面上說送到哪裡。
export const generateStrings = {
  "info.generate": "產生譯本",

  // ---------- 備份檔（設定的文獻庫區） ----------
  "backup.about": "把整個文獻庫打包成一個檔案",
  "backup.export": "匯出備份檔",
  "backup.restore": "從備份檔還原",
  "backup.export.busy": "正在匯出備份檔…",
  "backup.restore.busy": "正在從備份檔還原…",
  "backup.export.failed.title": "沒有匯出備份檔",
  "backup.restore.failed.title": "沒有從備份檔還原",

  "backup.deliver.title": "備份檔做好了",
  "backup.deliver.summary": "「{name}」：{files} 個檔案，共 {size}。",
  "backup.deliver.temp": "還沒存下來。",
  "backup.deliver.skipped": "{n} 個檔案沒有放進備份檔",
  "backup.deliver.skippedLine": "{path}：{reason}",
  "backup.deliver.changed": "{n} 個檔案在匯出時被改動，可能不是最新的。",
  "backup.deliver.later": "先不存",
  "backup.deliver.failed": "沒有交出去。",
  "backup.deliver.saved.title": "已存好",
  "backup.deliver.shared.title": "已交給分享",
  "backup.deliver.file": "「{name}」",

  "backup.skip.non-utf8-name": "檔名不是正常的文字",
  "backup.skip.unsafe-path": "位置會指到文獻庫外面",
  "backup.skip.case-duplicate": "和另一個檔案只差在大小寫",
  "backup.skip.symlink": "是捷徑（符號連結）",
  "backup.skip.not-a-file": "不是一般的檔案",
  "backup.skip.unreadable": "讀不了",
  "backup.skip.other": "這個版本不認得的原因",

  "backup.confirm.title": "從備份檔還原？",
  "backup.confirm.body": "會用備份檔做出新的文獻庫並換過去；目前的文獻庫會留著。",
  "backup.confirm.time": "還原時請不要離開應用。",
  "backup.confirm.go": "選擇備份檔",

  "backup.result.title": "已從備份檔還原",
  "backup.result.summary": "還原了 {files} 個檔案，共 {size}。",
  "backup.result.old": "原本的文獻庫還在 {path}",
  "backup.result.new": "新的文獻庫在 {path}",
  "backup.result.problems": "有 {n} 個地方需要留意。",
  "backup.result.mismatches": "{n} 個檔案和備份檔對不上；原本的文獻庫還在。",

  "backup.size.b": "{n} B",
  "backup.size.kb": "{n} KB",
  "backup.size.mb": "{n} MB",
  "backup.size.gb": "{n} GB",

  // ---------- 產生畫面 ----------
  "generate.paper": "這篇文獻：{title}",
  "generate.confirm.sends.none": "不送出任何東西。",

  // 用量：服務回報的詞元數，照服務設定裡填的單價粗估花費
  "generate.usage.label": "用量",
  "generate.usage.this": "這次：輸入 {input}、輸出 {output} 詞元",
  "generate.usage.paper": "這篇累計（這支手機）：輸入 {input}、輸出 {output} 詞元",
  "generate.usage.cost": "（約 US${cost}）",
  "generate.usage.costNote": "費用照服務設定裡填的單價粗估。",
  "generate.usage.noPrice": "服務設定裡沒有填單價，只顯示詞元數。",
} as const;
