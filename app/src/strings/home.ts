// 介面文字（home 區：首頁、側邊欄的群組、文獻資訊的群組與譯本）。用詞：群組、未分組、側邊欄、譯本。
export const homeStrings = {
  // 首頁的版面
  "home.section.recent": "最近加入",
  "home.section.group": "這個群組的文獻",
  "home.section.ungrouped": "還沒分組的文獻",
  "home.rail.label": "最近加入的文獻",
  "home.continue.go": "繼續",
  "home.continue.pageOf": "第 {page}／{total} 頁",
  "home.status.pageOf": "讀到第 {page}／{total} 頁",
  "home.row.menu": "「{title}」的選項",
  "home.row.digest": "筆",
  "home.row.digestFull": "有重點筆記",
  "home.row.translation": "譯",

  // 篩選（閱讀狀態之外，依內容）
  "home.filter.translation": "有譯本",
  "home.filter.digest": "有重點筆記",
  "home.filter.notes": "有筆記",

  // 側邊欄
  "sidebar.ungrouped": "未分組",
  "sidebar.newGroup": "新增群組",
  "sidebar.groupMenu": "「{name}」的選項",

  // 群組
  "group.rename": "改名",
  "group.delete": "刪除群組",
  "group.name.renameTitle": "群組改名",
  "group.name.label": "群組名稱",
  "group.name.placeholder": "例如：物件偵測",
  "group.name.create": "建立",
  "group.name.empty": "請輸入名稱。",
  "group.name.tooLong": "名稱最多 100 個字。",
  "group.name.duplicate": "已經有同名的群組。",
  "group.name.memberFailed": "群組已建立，但這篇沒有放進去。",
  "group.delete.title": "刪除這個群組？",
  "group.delete.body": "裡面的文獻不會被刪除。",
  "group.delete.failed.title": "沒有刪除群組",
  "group.delete.skipped.title": "群組已刪除",
  "group.delete.skipped": "{n} 篇的群組記錄暫時沒更新，不影響閱讀。",
  "group.pick.title": "放進群組",
  "group.none": "還沒有群組。",
  "group.pick.failed.title": "沒有變更群組",
  "group.empty.title": "這個群組還沒有文獻",
  "group.empty.body": "放進這個群組的文獻會出現在這裡。",
  "group.ungrouped.empty.title": "沒有未分組的文獻",
  "group.ungrouped.empty.body": "每一篇都已經分組了。",
  "group.empty.back": "看全部文獻",

  // 譯本
  "translation.add": "加入譯本",
  "translation.has": "有譯本",
  "translation.busy": "正在加入譯本…",
  "translation.added": "已加入譯本",
  "translation.present": "這份檔案原本就在這篇裡",
  "translation.failed.title": "沒有加入譯本",

  // 文獻資訊
  "info.groups.heading": "群組",
} as const;
