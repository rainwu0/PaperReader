// 譯本 PDF 用的字型：把系統的中日韓字型（Android：`/system/fonts/` 的 Noto CJK）只嵌入用到的字。
//
// 三種字型檔：
//   - `CFF2`（Android 16 的 Noto Sans CJK，可變字型）：fontkit 讀不了，用 `cff2.ts` 轉成 CFF1 子集，自己組 PDF 的字型物件；
//   - `glyf`（TrueType）與 `CFF `（CFF1）：交給 pdf-lib（它的字型子集套件 fontkit）嵌入。
// 字型檔本身不進應用也不進版本庫，由外殼唯讀交給畫面。

import fontkit from "@pdf-lib/fontkit";
import {
  PDFDocument,
  PDFHexString,
  PDFName,
  PDFOperator,
  PDFNumber,
  PDFOperatorNames,
  PDFString,
  type PDFFont,
  type PDFPage,
  type PDFRef,
  popGraphicsState,
  pushGraphicsState,
  setCharacterSpacing,
  setFillingRgbColor,
} from "pdf-lib";
import { cff2ToCff1Subset } from "./cff2.ts";

/** 一行字（不換行）要畫的位置與樣子。 */
export interface LineStyle {
  size: number;
  /** 每個字之外多加的間距（點），拿來左右對齊。 */
  charSpace: number;
  color: [number, number, number];
}

export interface Typeface {
  /** 一個字的寬度（千分之一個字級）；字型沒有這個字回傳 `null`。 */
  advance(cp: number): number | null;
  /** 畫一行字，`(x, y)` 是基線的左端。沒有的字略過。 */
  drawLine(page: PDFPage, text: string, x: number, y: number, style: LineStyle): void;
  /** 全部頁面畫完之後呼叫：把用到的字寫進字型物件。 */
  finish(): Promise<void>;
  /** 這份字型的名稱（寫進結果與錯誤訊息）。 */
  readonly name: string;
}

export class FontError extends Error {}

/** 目標語言 → Noto CJK 的字面後綴（TTC 裡一個字型檔有好幾個字面）。 */
export function faceSuffix(lang: string): string {
  const l = lang.toLowerCase();
  if (l === "zh-cn" || l === "zh-hans" || l === "zh-sg" || l === "zh") return "SC";
  if (l === "zh-hk" || l === "zh-mo") return "HK";
  if (l.startsWith("ja")) return "JP";
  if (l.startsWith("ko")) return "KR";
  return "TC";
}

/** 字型檔裡所有字面的 PostScript 名稱（單一字型檔就一個）。 */
export function faceNames(data: Uint8Array): string[] {
  const f = fontkit.create(data as never) as unknown as { fonts?: Array<{ postscriptName: string }>; postscriptName?: string };
  if (f.fonts) return f.fonts.map((x) => x.postscriptName);
  return f.postscriptName ? [f.postscriptName] : [];
}

interface KitFont {
  postscriptName: string;
  unitsPerEm: number;
  ascent: number;
  descent: number;
  capHeight: number;
  italicAngle: number;
  bbox: { minX: number; minY: number; maxX: number; maxY: number };
  directory: { tables: Record<string, { offset: number; length: number } | undefined> };
  glyphForCodePoint(cp: number): { id: number; advanceWidth: number };
  hasGlyphForCodePoint(cp: number): boolean;
}

function openFace(data: Uint8Array, lang: string): KitFont {
  const names = faceNames(data);
  const want = faceSuffix(lang);
  const pick = names.find((n) => n.toUpperCase().includes(want)) ?? names[0];
  if (!pick) throw new FontError("no face in font file");
  return fontkit.create(data as never, names.length > 1 ? pick : undefined) as unknown as KitFont;
}

const hex4 = (n: number) => n.toString(16).padStart(4, "0");

// ---------- CFF2：自己組字型物件 ----------

class Cff2Typeface implements Typeface {
  readonly name: string;
  private readonly fontRef: PDFRef;
  private readonly fontName: string;
  /** 用到的字：Unicode → 新字形編號（從 1 起）。 */
  private readonly used = new Map<number, number>();
  private readonly origGids: number[] = [];
  private readonly widths = new Map<number, number>();
  private readonly registered = new WeakSet<PDFPage>();
  private readonly doc: PDFDocument;
  private readonly data: Uint8Array;
  private readonly face: KitFont;

  constructor(doc: PDFDocument, data: Uint8Array, face: KitFont) {
    this.doc = doc;
    this.data = data;
    this.face = face;
    this.name = face.postscriptName;
    this.fontRef = doc.context.nextRef();
    this.fontName = `PRCJK${Math.random().toString(36).slice(2, 8).toUpperCase()}+${face.postscriptName}`.replace(/[^A-Za-z0-9+_-]/g, "");
  }

  advance(cp: number): number | null {
    if (!this.face.hasGlyphForCodePoint(cp)) return null;
    return (this.face.glyphForCodePoint(cp).advanceWidth * 1000) / this.face.unitsPerEm;
  }

  private gidOf(cp: number): number {
    const have = this.used.get(cp);
    if (have !== undefined) return have;
    const g = this.face.glyphForCodePoint(cp);
    const id = this.used.size + 1;
    this.used.set(cp, id);
    this.origGids.push(g.id);
    this.widths.set(id, (g.advanceWidth * 1000) / this.face.unitsPerEm);
    return id;
  }

  drawLine(page: PDFPage, text: string, x: number, y: number, st: LineStyle): void {
    let codes = "";
    for (const ch of text) {
      const cp = ch.codePointAt(0) ?? 0;
      if (!this.face.hasGlyphForCodePoint(cp)) continue;
      codes += hex4(this.gidOf(cp));
    }
    if (codes === "") return;
    if (!this.registered.has(page)) {
      page.node.setFontDictionary(PDFName.of("PRTr"), this.fontRef);
      this.registered.add(page);
    }
    const [r, g, b] = st.color;
    page.pushOperators(
      pushGraphicsState(),
      setFillingRgbColor(r, g, b),
      setCharacterSpacing(st.charSpace),
      PDFOperator.of(PDFOperatorNames.BeginText),
      PDFOperator.of(PDFOperatorNames.SetFontAndSize, [PDFName.of("PRTr"), PDFNumber.of(st.size)]),
      PDFOperator.of(PDFOperatorNames.MoveText, [PDFNumber.of(x), PDFNumber.of(y)]),
      PDFOperator.of(PDFOperatorNames.ShowText, [PDFHexString.of(codes)]),
      PDFOperator.of(PDFOperatorNames.EndText),
      popGraphicsState(),
    );
  }

  async finish(): Promise<void> {
    const ctx = this.doc.context;
    const table = this.face.directory.tables["CFF2"];
    if (!table) throw new FontError("CFF2 table missing");
    const sub = cff2ToCff1Subset(this.data.subarray(table.offset, table.offset + table.length), this.origGids);
    const scale = 1000 / this.face.unitsPerEm;
    const file = ctx.flateStream(sub.cff, { Subtype: "CIDFontType0C" });
    const fileRef = ctx.register(file);
    const bb = this.face.bbox;
    const descriptor = ctx.register(
      ctx.obj({
        Type: "FontDescriptor",
        FontName: this.fontName,
        Flags: 4, // 符號字型（沒有標準編碼）
        FontBBox: [bb.minX * scale, bb.minY * scale, bb.maxX * scale, bb.maxY * scale],
        ItalicAngle: this.face.italicAngle || 0,
        Ascent: this.face.ascent * scale,
        Descent: this.face.descent * scale,
        CapHeight: (this.face.capHeight || this.face.ascent) * scale,
        StemV: 80,
        FontFile3: fileRef,
      }),
    );
    // 寬度：新字形編號 1…n 連續，一個區段
    const ws: number[] = [];
    for (let id = 1; id <= this.used.size; id++) ws.push(Math.round(this.widths.get(id) ?? 1000));
    const cid = ctx.register(
      ctx.obj({
        Type: "Font",
        Subtype: "CIDFontType0",
        BaseFont: this.fontName,
        CIDSystemInfo: { Registry: PDFString.of("Adobe"), Ordering: PDFString.of("Identity"), Supplement: 0 },
        FontDescriptor: descriptor,
        DW: 1000,
        W: [1, ws],
      }),
    );
    // 複製文字用的對照：新字形編號 → Unicode
    const lines: string[] = [];
    for (const [cp, id] of this.used) {
      const u = cp > 0xffff ? hex4(0xd800 + ((cp - 0x10000) >> 10)) + hex4(0xdc00 + ((cp - 0x10000) & 0x3ff)) : hex4(cp);
      lines.push(`<${hex4(id)}> <${u}>`);
    }
    const chunks: string[] = [];
    for (let i = 0; i < lines.length; i += 100) {
      const part = lines.slice(i, i + 100);
      chunks.push(`${part.length} beginbfchar\n${part.join("\n")}\nendbfchar`);
    }
    const cmap =
      "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <ffff>\nendcodespacerange\n" +
      chunks.join("\n") +
      "\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend";
    const toUnicode = ctx.register(ctx.flateStream(cmap));
    ctx.assign(
      this.fontRef,
      ctx.obj({
        Type: "Font",
        Subtype: "Type0",
        BaseFont: this.fontName,
        Encoding: "Identity-H",
        DescendantFonts: [cid],
        ToUnicode: toUnicode,
      }),
    );
  }
}

// ---------- glyf / CFF1：交給 pdf-lib ----------

class PdfLibTypeface implements Typeface {
  readonly name: string;
  private readonly registered = new WeakSet<PDFPage>();
  private readonly font: PDFFont;
  private readonly face: KitFont;

  constructor(font: PDFFont, face: KitFont) {
    this.font = font;
    this.face = face;
    this.name = face.postscriptName;
  }

  advance(cp: number): number | null {
    if (!this.face.hasGlyphForCodePoint(cp)) return null;
    return (this.face.glyphForCodePoint(cp).advanceWidth * 1000) / this.face.unitsPerEm;
  }

  drawLine(page: PDFPage, text: string, x: number, y: number, st: LineStyle): void {
    let kept = "";
    for (const ch of text) if (this.face.hasGlyphForCodePoint(ch.codePointAt(0) ?? 0)) kept += ch;
    if (kept === "") return;
    if (!this.registered.has(page)) {
      page.node.setFontDictionary(PDFName.of("PRTr"), this.font.ref);
      this.registered.add(page);
    }
    const [r, g, b] = st.color;
    page.pushOperators(
      pushGraphicsState(),
      setFillingRgbColor(r, g, b),
      setCharacterSpacing(st.charSpace),
      PDFOperator.of(PDFOperatorNames.BeginText),
      PDFOperator.of(PDFOperatorNames.SetFontAndSize, [PDFName.of("PRTr"), PDFNumber.of(st.size)]),
      PDFOperator.of(PDFOperatorNames.MoveText, [PDFNumber.of(x), PDFNumber.of(y)]),
      PDFOperator.of(PDFOperatorNames.ShowText, [this.font.encodeText(kept)]),
      PDFOperator.of(PDFOperatorNames.EndText),
      popGraphicsState(),
    );
  }

  async finish(): Promise<void> {
    // pdf-lib 在存檔時才寫字型子集
  }
}

/** 依字型檔的種類打開，交回可以畫字的 `Typeface`。 */
export async function openTypeface(doc: PDFDocument, data: Uint8Array, lang: string): Promise<Typeface> {
  const face = openFace(data, lang);
  if (face.directory.tables["CFF2"]) return new Cff2Typeface(doc, data, face);
  const names = faceNames(data);
  const pick = names.find((n) => n.toUpperCase().includes(faceSuffix(lang))) ?? names[0];
  doc.registerFontkit({
    create: (d: Uint8Array) => fontkit.create(d as never, names.length > 1 ? pick : undefined),
  } as never);
  const font = await doc.embedFont(data, { subset: true });
  return new PdfLibTypeface(font, face);
}
