// 介面文字（離線翻譯：本機模型 Firefox 翻譯、下載與刪除語言模型）。
// 下載模型會連網：下載的確認框留在畫面上說連到哪裡、下載多大、不送出什麼。
export const ondeviceStrings = {
  "settings.generate.provider.bergamot": "Firefox 翻譯（離線）",
  "ondevice.ready": "已下載，{size}",
  "ondevice.notReady": "還沒下載（約 {size}）",
  "ondevice.noLang": "沒有譯成「{lang}」的模型。",
  "ondevice.otherModels": "其他已下載的模型：{list}",
  "ondevice.download": "下載模型",
  "ondevice.delete": "刪除模型",
  "ondevice.deleteOther": "刪除 {name}",
  "ondevice.deleted": "模型已刪除，要用時可以再下載。",
  "ondevice.downloaded": "模型已下載好。",
  "ondevice.downloading": "正在下載… {done} / {total}",
  "ondevice.cancelDownload": "停止下載",
  "ondevice.ask.title": "下載語言模型？",
  "ondevice.ask.body": "會從 {host} 下載約 {size} 的模型檔，存在這支手機上；不送出任何文獻內容。",
  "ondevice.ask.confirm": "下載",
  "ondevice.wifiOnly": "只在 Wi-Fi 下載模型",
  "ondevice.failed": "沒有完成：{reason}",

  // 產生畫面
  "generate.local.needModel": "先下載語言模型。",

  // 版面模型（只用在翻譯的分段）：下載、刪除的文字和上面的語言模型共用，這裡只放不同的幾句
  "layout.title": "版面模型",
  "layout.about": "只用來把譯本分段分得更準；沒下載時照現行的方式。",
  "layout.ask.title": "下載版面模型？",
} as const;
