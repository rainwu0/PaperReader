// 驗收用的 PDF 產生器（只用 Node 內建功能，不加任何套件）：可以指定自帶的篇名與作者、大綱。
// 產生的檔案放在系統暫存資料夾，不進版本庫。
//
// 用途：A7（讀出 PDF 自帶的篇名與作者，好的採用、垃圾的不採用）、B2（有大綱與沒有大綱）。

const esc = (s) => String(s).replace(/\\/g, "\\\\").replace(/\(/g, "\\(").replace(/\)/g, "\\)");

/**
 * @param {object} o
 * @param {number} o.pages 頁數
 * @param {string} o.label 每頁大標題的字
 * @param {string} [o.title] Info 的 Title
 * @param {string} [o.author] Info 的 Author
 * @param {Array<{title: string, page: number, children?: any[]}>} [o.outline] 大綱（可以有層級）
 * @returns {Buffer}
 */
export function buildPdf({ pages, label, title, author, outline }) {
  const objs = []; // 1-based：objs[n - 1] 是第 n 個物件的內容
  const add = (body) => objs.push(body) && objs.length;
  const reserve = () => {
    objs.push(null);
    return objs.length;
  };
  const catalog = reserve();
  const pagesObj = reserve();
  const font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
  const pageRefs = [];
  for (let p = 1; p <= pages; p++) {
    const text = `BT /F1 24 Tf 60 760 Td (${esc(label)} - page ${p}) Tj ET\nBT /F1 11 Tf 60 720 Td 14 TL ${Array.from(
      { length: 40 },
      (_, i) => `(Line ${i + 1} of page ${p}: the quick brown fox jumps over the lazy dog.) Tj T*`,
    ).join(" ")} ET`;
    const content = add(`<< /Length ${text.length} >>\nstream\n${text}\nendstream`);
    pageRefs.push(
      add(
        `<< /Type /Page /Parent ${pagesObj} 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 ${font} 0 R >> >> /Contents ${content} 0 R >>`,
      ),
    );
  }
  objs[pagesObj - 1] = `<< /Type /Pages /Count ${pages} /Kids [${pageRefs.map((r) => `${r} 0 R`).join(" ")}] >>`;

  let outlinesRef = null;
  if (outline && outline.length > 0) {
    outlinesRef = reserve();
    // 先替每一項配好物件編號，再寫內容（項目之間要互相指到）
    const alloc = (items) => items.map((it) => ({ ...it, ref: reserve(), kids: alloc(it.children ?? []) }));
    const tree = alloc(outline);
    const count = (items) => items.reduce((n, it) => n + 1 + count(it.kids), 0);
    const write = (items, parent) => {
      items.forEach((it, i) => {
        const parts = [`/Title (${esc(it.title)})`, `/Parent ${parent} 0 R`];
        if (i > 0) parts.push(`/Prev ${items[i - 1].ref} 0 R`);
        if (i < items.length - 1) parts.push(`/Next ${items[i + 1].ref} 0 R`);
        if (it.kids.length > 0) {
          parts.push(`/First ${it.kids[0].ref} 0 R`, `/Last ${it.kids[it.kids.length - 1].ref} 0 R`, `/Count ${count(it.kids)}`);
        }
        parts.push(`/Dest [${pageRefs[it.page - 1]} 0 R /XYZ null null null]`);
        objs[it.ref - 1] = `<< ${parts.join(" ")} >>`;
        write(it.kids, it.ref);
      });
    };
    write(tree, outlinesRef);
    objs[outlinesRef - 1] = `<< /Type /Outlines /First ${tree[0].ref} 0 R /Last ${tree[tree.length - 1].ref} 0 R /Count ${count(tree)} >>`;
  }
  objs[catalog - 1] = `<< /Type /Catalog /Pages ${pagesObj} 0 R${outlinesRef ? ` /Outlines ${outlinesRef} 0 R` : ""} >>`;

  let infoRef = null;
  if (title !== undefined || author !== undefined) {
    const parts = [];
    if (title !== undefined) parts.push(`/Title (${esc(title)})`);
    if (author !== undefined) parts.push(`/Author (${esc(author)})`);
    infoRef = add(`<< ${parts.join(" ")} >>`);
  }

  let out = "%PDF-1.7\n";
  const offsets = [];
  objs.forEach((body, i) => {
    offsets.push(Buffer.byteLength(out, "latin1"));
    out += `${i + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xref = Buffer.byteLength(out, "latin1");
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
  for (const off of offsets) out += `${String(off).padStart(10, "0")} 00000 n \n`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root ${catalog} 0 R${infoRef ? ` /Info ${infoRef} 0 R` : ""} >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(out, "latin1");
}

/** 驗收用的幾份 PDF：檔名（英文，adb 在 Windows 上傳中文檔名會出問題）→ 內容。 */
export const EXTRA_SAMPLES = {
  // 好的篇名與作者、有三層的大綱
  "Outlined-Paper.pdf": buildPdf({
    pages: 12,
    label: "Outlined Paper",
    title: "Attention Is All You Need: A Survey",
    author: "Ashish Vaswani; Noam Shazeer",
    outline: [
      { title: "1 Introduction", page: 1 },
      {
        title: "2 Background",
        page: 3,
        children: [
          { title: "2.1 Related Work", page: 4 },
          { title: "2.2 Notation", page: 5 },
        ],
      },
      {
        title: "3 Method",
        page: 6,
        children: [
          { title: "3.1 Encoder", page: 7, children: [{ title: "3.1.1 Self-attention", page: 8 }] },
          { title: "3.2 Decoder", page: 9 },
        ],
      },
      { title: "4 Results", page: 10 },
      { title: "5 Conclusion", page: 12 },
    ],
  }),
  // 垃圾的篇名與作者（文書軟體留下的）、沒有大綱
  "Junk-Meta.pdf": buildPdf({
    pages: 6,
    label: "Junk Meta",
    title: "Microsoft Word - draft final.docx",
    author: "Administrator",
  }),
};
