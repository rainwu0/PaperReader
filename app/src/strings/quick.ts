// 介面文字（選取文字的快速翻譯）：選字之後的「翻譯」、浮在選取上方的卡片、設定裡的「快速翻譯用的服務」。
// 用詞：劃線、短評、譯文、原文、翻譯服務。
export const quickStrings = {
  // 選字之後的鈕
  "quick.action": "翻譯",

  // 卡片
  "quick.card": "快速翻譯",
  "quick.service": "用哪個服務",
  "quick.sameAsFull": "跟全文翻譯相同（{name}）",
  "quick.loading": "翻譯中…",
  "quick.offline": "沒有網路，改用 {name}",
  "quick.cached": "查過的，沒有再送出",
  "quick.copy": "複製",
  "quick.copied": "已複製譯文",
  "quick.copyFailed": "沒有複製到：這台裝置不讓應用寫入剪貼簿。",
  "quick.toComment": "劃線並寫進短評",
  "quick.savedAsComment": "已劃線，譯文寫進了短評。",
  "quick.retry": "再試一次",
  "quick.useOther": "改用 {name}",
  "quick.failed": "沒有翻成：{reason}",
  "quick.noService": "沒有可以用的翻譯服務。到設定 → 翻譯服務啟用一個。",
  "quick.unsupported": "{name} 不支援譯成目前的目標語言。",
  "quick.missing": "{name}：{why}",
  "quick.tooLong": "選了 {n} 字，快速翻譯最多 2,000 字。整頁、整節請用全文翻譯。",
  "quick.toFull": "全文翻譯",
  "quick.consent": "會把選取的文字與前後各一句送到 {host}。",
  "quick.consentNote": "這個服務之後快速翻譯不再問；可以在設定 → 產生 → 快速翻譯重設。",
  "quick.send": "送出",
  "quick.close": "關閉",

  // 設定
  "quick.settings.title": "快速翻譯用的服務",
  "quick.settings.hint": "選字之後按「翻譯」用的服務；卡片角落也可以換，換了就記住。",
  "quick.settings.consents": "已同意送出的服務：{n} 個",
  "quick.settings.resetConsents": "重新詢問",
  "quick.settings.consentsReset": "之後快速翻譯送出前會再問一次。",
} as const;
