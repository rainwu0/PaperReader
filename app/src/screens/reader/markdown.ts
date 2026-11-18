// 筆記預覽用的 Markdown 轉換（micromark，MIT）。預設不輸出原始 HTML：筆記是使用者的檔案，
// 也可能被別的工具改過，預覽不執行裡面的標籤。單獨的換行在樣式裡顯示成換行（`.md-preview`），
// 和 Obsidian 預設的樣子一致。
import { micromark } from "micromark";

export function renderMarkdown(text: string): string {
  return micromark(text);
}
