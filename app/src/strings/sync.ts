// 介面文字（同步區：同步頁、匯入的預覽、從另一台加入、合併沒做完）。用詞：
// 同步包、匯出同步包、匯入同步包（介面上一律說全）、合併、衝突、預覽、加入（文獻庫）、衝突副本。
// 寫法：版面只放結論；解釋放「詳情」。
export const syncStrings = {
  // 設定與同步頁
  "settings.sync": "同步",
  "sync.title": "同步",
  "sync.lead": "在一台匯出同步包，到另一台匯入，兩邊的修改會合併。",
  "sync.full": "完整的同步包",
  "sync.full.hint": "預設只帶變更",
  "sync.export": "匯出同步包",
  "sync.import": "匯入同步包",
  "sync.details.clock": "兩台裝置的時間要設成自動，才分得出哪邊的修改較新。",
  "sync.details.delete": "要刪劃線與文獻，請在應用裡刪，另一台才會跟著刪。",
  "sync.details.samePdf": "同一份 PDF 在兩台各加一次，會變成兩篇。",

  // 匯出
  "sync.export.busy": "正在匯出同步包…",
  "sync.export.failed.title": "沒有匯出同步包",
  "sync.deliver.title": "同步包做好了",
  "sync.deliver.summary": "「{name}」，共 {size}。",
  "sync.deliver.becameFull": "另一台還沒匯入過這台的同步包，這份是完整的。",
  "sync.deliver.skipped": "{n} 個檔案沒有放進同步包",

  // 匯入與預覽
  "sync.import.busy": "正在讀取同步包…",
  "sync.import.failed.title": "沒有匯入同步包",
  "sync.busy.percent": "{label} {pct}%",
  "sync.preview.title": "匯入同步包",
  "sync.preview.from": "來自 {device}，{when} 匯出",
  "sync.preview.otherDevice": "另一台裝置",
  "sync.preview.already": "這份同步包已經匯入過。",
  "sync.preview.nothing": "沒有要合併的變更。",
  "sync.preview.added": "新增",
  "sync.preview.updated": "更新",
  "sync.preview.removed": "移除",
  "sync.preview.byPaper": "逐篇看",
  "sync.preview.paperLine": "{title}：{what}",
  "sync.preview.conflicts": "衝突（{n}）",
  "sync.preview.notices": "提醒",
  "sync.preview.skipped": "這次不合併的文獻",
  "sync.preview.merge": "合併",
  "sync.list.sep": "、",

  // 彙總的種類
  "sync.item.Paper": "文獻 {n} 篇",
  "sync.item.Material": "檔案 {n} 份",
  "sync.item.Bib": "書目 {n} 篇",
  "sync.item.Groups": "群組所屬 {n} 篇",
  "sync.item.Pages": "頁數 {n} 篇",
  "sync.item.Highlight": "劃線 {n} 條",
  "sync.item.Thoughts": "整篇的想法 {n} 篇",
  "sync.item.Stroke": "筆跡 {n} 筆",
  "sync.item.Overlay": "譯文 {n} 份",
  "sync.item.NotesFile": "筆記檔 {n} 份",
  "sync.item.Group": "群組 {n} 個",
  "sync.item.TrashState": "回收區 {n} 篇",
  "sync.item.References": "參考文獻清單 {n} 份",
  "sync.item.Text": "抽出的文字 {n} 份",
  "sync.item.other": "其他 {n} 項",

  // 衝突
  "sync.conflict.Bib": "書目兩邊都改過",
  "sync.conflict.HighlightEdited": "劃線的短評兩邊都改過",
  "sync.conflict.HighlightDeletedButEdited": "一邊刪了這條劃線，另一邊改過",
  "sync.conflict.Thoughts": "整篇的想法兩邊都改過",
  "sync.conflict.MaterialFile": "同一份檔案兩邊不同",
  "sync.conflict.Overlay": "譯文兩邊都改過（{n} 段）",
  "sync.conflict.TrashState": "一邊在回收區，另一邊在目錄",
  "sync.conflict.other": "兩邊都改過",
  "sync.side.local": "這台",
  "sync.side.deleted": "已刪除",
  "sync.side.empty": "（空白）",
  "sync.side.papers": "在目錄",
  "sync.side.trash": "在回收區",
  "sync.side.when": "{when} 改的",
  "sync.side.whenApprox": "約 {when} 改的",
  "sync.side.field": "{field}：{value}",
  "sync.field.title": "篇名",
  "sync.field.authors": "作者",
  "sync.field.year": "年份",
  "sync.field.venue": "出處",
  "sync.field.doi": "DOI",
  "sync.field.url": "網址",
  "sync.choice.Both": "兩個都留",

  // 提醒（一種一句）
  "sync.notice.SameSource": "兩台各加了同一份 PDF，會有兩篇。",
  "sync.notice.SameGroupName": "兩台各建了同名的群組，會有兩個。",
  "sync.notice.ConflictCopy": "有筆記另存成衝突副本。",
  "sync.notice.NotesOutsideText": "筆記檔裡標記以外的文字兩邊不同，另存了衝突副本。",
  "sync.notice.IncomingUnreadable": "另一台有檔案讀不懂，略過了。",
  "sync.notice.NewerFormat": "另一台有檔案是較新版本的應用做的，略過了。",
  "sync.notice.LocalUnreadable": "這台有檔案讀不懂，那部分沒有合併。",
  "sync.notice.UnmergedWrite": "這些檔在合併時已被別處改過，沒有覆寫；筆記的合併結果另存在旁邊。",
  "sync.notice.MaterialChangedOutside": "這份 PDF 在應用以外被改過，沒有用另一台的那份蓋掉。",
  "sync.notice.MissingFile": "有一份材料沒有帶過來，請在另一台匯出完整的同步包。",
  "sync.notice.TooLargeToMerge": "有一篇的參考文獻清單合起來會超過上限，沒有合併，這台的照舊；清掉一些找不回的判斷之後再同步一次。",
  "sync.notice.TooLargeToMerge.text": "有一篇的抽出的文字超過上限，沒有合併，這台的照舊；這台沒有的會自己重新抽。",
  "sync.notice.PurgedButRestored": "有一篇在另一台永久刪除了，但還在目錄裡（放回了或一直在用），所以留著沒刪。",
  "sync.notice.other": "有一項這個版本不認得的提醒。",
  "sync.notice.count": "（{n} 則）",

  // 不合併的文獻的原因
  "sync.skip.missing-file": "缺檔，請在另一台匯出完整的同步包",
  "sync.skip.format-too-new": "是較新版本的應用做的",
  "sync.skip.unreadable": "讀不懂",
  "sync.skip.other": "這個版本不認得的原因",

  // 合併
  "sync.apply.busy": "正在合併…",
  "sync.apply.failed.title": "沒有合併",
  "sync.apply.expired": "預覽之後這台又改過，請重新匯入同步包。",
  "sync.apply.again": "重新匯入",
  "sync.result.title": "合併好了",
  "sync.result.summary": "新增 {added} 項、更新 {updated} 項、移除 {removed} 項。",
  "sync.result.copies": "{n} 份衝突副本放在筆記檔旁邊",
  "sync.result.skipped": "{n} 篇這次沒有合併。",
  "sync.result.problems": "有 {n} 個地方需要留意。",

  // 合併沒做完
  "sync.unfinished": "上一次的合併沒做完，文獻庫暫時只能看。",
  "sync.unfinished.retry": "重試",
  "sync.unfinished.busy": "正在把合併做完…",
  "sync.unfinished.failed.title": "合併還沒做完",
  "sync.unfinished.done": "合併做完了",
  "sync.unfinished.doneBody": "文獻庫可以照常使用了。",
  "problem.UnfinishedSyncImport": "上一次的合併沒做完，文獻庫暫時只能看。",

  // 從另一台加入（第一次開啟、文獻庫是空的時）
  "join.action": "從另一台加入",
  "join.title": "從另一台加入",
  "join.hint": "請用另一台剛匯出的同步包或備份檔。",
  "join.pick": "選擇檔案",
  "join.busy": "正在加入…",
  "join.failed.title": "沒有加入文獻庫",
  "join.done.title": "已加入",
  "join.done.summary": "{files} 個檔案，共 {size}。",
  "join.done.kept": "原本的文獻庫還在原處。",
  "join.done.where": "文獻庫在 {path}",

  // 從備份檔還原的位置
  "backup.confirm.bodyEmpty": "會用備份檔建立文獻庫。",
  "backup.result.kept": "原本的文獻庫還在原處。",

  // 閱讀：另一台的閱讀位置
  "reader.otherDevice": "另一台讀到第 {page} 頁",

  // 出錯
  "error.SyncOtherLibrary": "這份同步包屬於另一個文獻庫。",
  "error.SyncOtherLibrary.detail":
    "兩台要先用同一份同步包或備份檔「從另一台加入」，之後才能互相合併。文獻庫沒有任何改變。",
  "error.SyncOwnPackage": "這是這台裝置自己匯出的。",
  "error.SyncOwnPackage.detail": "請到另一台匯入它。文獻庫沒有任何改變。",
  "error.SyncBackupFile": "這是備份檔，請用「從備份檔還原」。",
  "error.SyncBackupFile.detail": "文獻庫沒有任何改變。",
  "error.SyncGotImportZip": "這是整批匯入用的 zip，請在首頁按「＋」，選「整批匯入」。",
  "error.SyncGotImportZip.detail": "它的根目錄有匯入清單 paperreader-import.json。文獻庫沒有任何改變。",
  "error.SyncIncomplete": "加入要用完整的同步包。",
  "error.SyncIncomplete.detail": "在另一台打開「完整的同步包」再匯出一次。",
  "error.NotBackupFile": "這不是備份檔。",
  "error.NotBackupFile.detail": "目前的文獻庫沒有任何改變。",
  "error.RestoreGotSyncPackage": "這是同步包，請在設定的「同步」匯入。",
  "error.RestoreGotSyncPackage.detail": "目前的文獻庫沒有任何改變。",

  // 開啟時自動做完上一次沒做完的合併
  "sync.resumed": "上一次沒做完的合併已經做完。",
} as const;
