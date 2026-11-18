// 介面文字（應用內檢查更新）。寫法：連到哪裡、送出什麼、會怎樣，留在做決定的畫面上。
export const updateStrings = {
  "update.title": "檢查更新",
  "update.settings.row": "檢查更新",
  "update.settings.available": "有新版本 {version}",

  // 同意
  "update.consent.lead": "PaperReader 可以到 GitHub 看看有沒有新版本。",
  "update.consent.where":
    "同意後，每次打開應用時，一天最多自動連到 {host} 問一次有沒有新版；每次是一個不帶任何資料的請求，GitHub 會看到你的 IP 位址。",
  "update.consent.download": "有新版時，你按「更新」才會從 GitHub 下載安裝檔，核對過才安裝。",
  "update.consent.off": "之後可以在這裡關掉自動檢查，或收回同意。",
  "update.consent.agree": "同意並檢查",
  "update.consent.decline": "不要",

  // 狀態
  "update.current": "目前的版本",
  "update.auto": "自動檢查新版本",
  "update.auto.hint": "打開應用時，一天最多問一次",
  "update.checkNow": "現在檢查",
  "update.checking": "正在問 GitHub…",
  "update.upToDate": "已是最新版本。",
  "update.lastCheck": "上次檢查：{when}",
  "update.new": "新版本 {version}",
  "update.size": "安裝檔 {size}",
  "update.notes.empty": "這一版沒有寫變更摘要。",
  "update.closes": "安裝時 PaperReader 會關閉，裝好後再打開；文獻庫不受影響。",
  "update.go": "更新",
  "update.downloading": "正在下載…",
  "update.downloadingPct": "正在下載… {pct}%",
  "update.verifying": "正在核對…",
  "update.install": "安裝",
  "update.installing": "交給系統安裝…",
  "update.installing.wait": "系統正在安裝。PaperReader 會關閉，裝好後請再打開。",
  "update.cancelled": "沒有下載，什麼都沒改。",
  "update.discard": "不裝了，刪掉下載的檔",

  // 安裝失敗時說得出原因：白話一句，下面是系統原文；中止照實說，不說成「你取消了」
  "update.fail.cancelled": "沒有安裝：確認畫面按了「取消」。",
  "update.fail.aborted": "系統中止了安裝。",
  "update.fail.invalid": "系統說安裝檔無效，沒有安裝。",
  "update.fail.conflict": "系統說安裝檔和已安裝的 PaperReader 衝突，沒有安裝。",
  "update.fail.storage": "手機空間不夠，系統沒有裝成。",
  "update.fail.incompatible": "系統說這個安裝檔和這台手機不相容，沒有安裝。",
  "update.fail.blocked": "系統擋下了安裝。",
  "update.fail.timeout": "系統安裝逾時，沒有裝成。",
  "update.fail.failed": "系統沒有裝成。",
  "update.fail.stalled": "系統沒有回報安裝的結果，新版沒有裝上。",
  "update.fail.stalled.note": "確認畫面一閃就不見、或按了「安裝」之後沒有反應，都會是這一句。",
  "update.fail.next": "下載的檔留著。可以再按一次「安裝」，或改用「用系統安裝程式開啟」。",
  "update.fail.aborted.note": "如果你剛才在確認畫面按的是「取消」，也會是這一句。",
  "update.fail.cancelled.note": "如果你按的其實是「安裝」，請複製錯誤資訊回報。",
  "update.fail.message": "系統的原話：{message}",
  "update.fail.noMessage": "系統沒有附上說明。",
  "update.fail.code": "代碼：{name}（{status}）",
  "update.fail.copy": "複製錯誤資訊",
  "update.fail.copied": "已複製。可以貼到 GitHub 的問題回報或訊息裡。",
  "update.fail.copyFailed": "沒有複製成功。可以展開下面「要複製的內容」截圖。",
  "update.fail.preview": "要複製的內容",
  "update.fail.private": "只有版本、系統的訊息、手機型號與系統版本，沒有個人資料；不會自動送出。",

  // 退路：用系統安裝程式開啟
  "update.installer": "用系統安裝程式開啟",
  "update.installer.hint": "改由系統的安裝程式裝同一個檔。這條路在有的手機上也會失敗；失敗時請截下系統畫面的文字。",
  "update.installer.back": "沒有裝上新版。如果系統的安裝程式顯示了錯誤，請截下那個畫面回報。",
  "update.installer.noApp": "這台手機找不到可以安裝的系統程式。",

  // 小米且「MIUI 最佳化」開著
  "update.miui":
    "在小米手機上，「MIUI 最佳化」開著時，系統可能中止應用內更新。失敗時畫面會寫出系統給的原因，也可以改按「用系統安裝程式開啟」。",

  // 「複製錯誤資訊」的內容（純文字；版本、系統的訊息、裝置與系統版本，不含個人資料）
  "update.report.title": "PaperReader 更新失敗的錯誤資訊",
  "update.report.time": "時間：{value}",
  "update.report.installed": "已安裝：{value}",
  "update.report.target": "要裝的：{value}",
  "update.report.result": "結果：{value}",
  "update.report.message": "系統訊息：{value}",
  "update.report.legacy": "舊式代碼：{value}",
  "update.report.confirm": "確認畫面：{value}",
  "update.report.timing": "時間差：確認畫面到結果 {confirm}；交給系統到結果 {commit}",
  "update.report.device": "裝置：{value}",
  "update.report.android": "Android：{value}",
  "update.report.build": "組建：{value}",
  "update.report.rom": "MIUI：{miui}；HyperOS：{hyper}",
  "update.report.optimization": "MIUI 最佳化：{value}",
  "update.report.installer": "安裝者紀錄：{value}",
  "update.report.unknownSources": "允許安裝不明應用程式：{value}",
  "update.report.ms": "{ms} 毫秒",
  "update.report.none": "（無）",
  "update.report.notXiaomi": "不是小米的系統",
  "update.report.unknown": "未知",
  "update.report.yes": "是",
  "update.report.no": "否",
  "update.report.on": "開",
  "update.report.off": "關",
  "update.report.initiator": "{installer}（由 {initiator} 發起）",

  // 收回同意
  "update.revoke": "收回同意",
  "update.revoke.hint": "不再連到 GitHub；查到的版本與下載的檔一起刪掉",
  "update.revoked": "已收回同意。PaperReader 不會再連到 GitHub 檢查更新。",

  // 行動數據
  "update.metered.title": "用行動數據下載？",
  "update.metered.body": "目前不是 Wi-Fi。安裝檔約 {size}。",
  "update.metered.go": "下載",

  // 安裝不明應用程式
  "update.permission.title": "允許 PaperReader 安裝更新",
  "update.permission.body": "第一次在應用裡更新，要在系統設定裡允許 PaperReader「安裝不明應用程式」。回到這裡就會接著安裝。",
  "update.permission.later": "之後在 Android 12 以上的手機，更新通常不必再按「安裝」。",
  "update.permission.go": "前往設定",
  "update.permission.denied": "還沒允許，沒有安裝。下載的檔留著，允許後再按「安裝」。",

  // 首頁
  "update.home.available": "有新版本 {version}。",
  "update.home.view": "查看",
  "update.home.skip": "略過這一版",
  "update.home.invite": "要在有新版時告訴你嗎？會連到 GitHub 查。",
  "update.home.invite.yes": "看看",
  "update.home.invite.no": "不用",

  // 錯誤
  "error.UpdateNetwork": "連不上 GitHub。",
  "error.UpdateNetwork.detail": "請確認有網路，稍後再試。",
  "error.UpdateRateLimited": "GitHub 暫時不回應，稍後再試。",
  "error.UpdateRateLimited.detail": "GitHub 對沒有登入的請求每小時有次數上限；同一個網路上別的程式也算在內。",
  "error.UpdateBadRelease": "GitHub 上的這一版不完整，沒有下載。",
  "error.UpdateBadRelease.detail": "找不到這台手機要的安裝檔或核對碼清單，或內容讀不懂。",
  "error.UpdateChecksum": "下載的檔和公布的核對碼不符，已刪掉，沒有安裝。",
  "error.UpdateChecksum.detail": "可能是下載途中壞掉。可以再試一次；一直不符時請到 GitHub 的發布頁回報。",
  "error.UpdateSignature": "下載的安裝檔不是用同一把金鑰簽的，已刪掉，沒有安裝。",
  "error.UpdateSignature.detail":
    "它可能不是 PaperReader 的正式版本，或版本不比現在新。為了安全，應用內更新只裝和已安裝的同一把金鑰簽的安裝檔。",
  "error.UpdateUnverifiable": "這台手機讀不出安裝檔的簽章，無法核對，已刪掉，沒有安裝。",
  "error.UpdateUnverifiable.detail":
    "這是部分 Android 版本的限制，不代表安裝檔有問題。請到 GitHub 的發布頁下載安裝檔，直接裝在舊版上。",
  "error.UpdateGone": "這一版已從 GitHub 撤下，沒有下載。",
  "error.UpdateGone.detail": "查到的版本與下載的檔都已清掉。可以稍後再檢查一次。",
  "error.UpdateInstall": "系統沒有裝成。",
  "error.UpdateInstall.detail": "可能是手機空間不夠，或系統拒絕。可以到 GitHub 的發布頁下載安裝檔，直接裝在舊版上。",
} as const;
