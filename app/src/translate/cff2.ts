// CFF2（Android 16 內建的 Noto Sans CJK 是這種字型：可變字型，字形放在 `CFF2` 表）轉成 PDF 看得懂的 CFF1。
//
// PDF 的字型檔只能放 TrueType（`glyf`）或 CFF1；pdf-lib 用的 fontkit 兩種都讀不了 CFF2，所以自己轉：
// 取出預設樣式（不套用任何變化軸）的字形，只留要用的字，輸出成 CID 形式的 CFF1，
// 在 PDF 裡當 `FontFile3`（`CIDFontType0C`）嵌入，CID 就是新字形的編號。
//
// 做的事：
//   - 字形的筆畫指令逐條讀過：子程式（callsubr／callgsubr）展開、`blend` 只留預設值、`vsindex` 吃掉；
//   - 提示（hstem、vstem、hintmask…）全部丟掉（畫面與 PDF.js 都不用提示）；
//   - 結尾補 CFF1 需要的 `endchar`。
// 不做：CFF1 字型（fontkit 本來就讀得了）、變化軸的套用；一條指令的引數超過 CFF1 的上限（48）就報錯
// （Android 16 的 Noto Sans CJK 全部 65,535 個字形最多用到 46，沒有碰到）。

export interface Cff1Subset {
  /** CFF1 表的位元組（放進 PDF 的 `FontFile3`）。 */
  cff: Uint8Array;
  /** 字形數，含 0 號 `.notdef`。 */
  glyphCount: number;
  /** 拆開之前，任何一條指令用過的最多引數（量測用）。 */
  maxArgs: number;
}

// ---------- 讀 CFF2 ----------

interface Index {
  count: number;
  /** 第 i 個物件的 [起, 迄)。 */
  at(i: number): [number, number];
  end: number;
}

function readIndex2(b: Uint8Array, at: number): Index {
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const count = dv.getUint32(at);
  if (count === 0) return { count: 0, at: () => [0, 0], end: at + 4 };
  const offSize = b[at + 4] ?? 0;
  const offsetsAt = at + 5;
  const dataStart = offsetsAt + (count + 1) * offSize - 1;
  const off = (i: number) => {
    let v = 0;
    for (let k = 0; k < offSize; k++) v = v * 256 + (b[offsetsAt + i * offSize + k] ?? 0);
    return v;
  };
  return {
    count,
    at: (i) => [dataStart + off(i), dataStart + off(i + 1)],
    end: dataStart + off(count),
  };
}

type Dict = Map<number, number[]>;

/** DICT 的運算子編成數字：單位元組的就是它自己，`12 x` 是 `1200 + x`。 */
function parseDict(b: Uint8Array, from: number, to: number): Dict {
  const out: Dict = new Map();
  let args: number[] = [];
  let i = from;
  while (i < to) {
    const b0 = b[i++] ?? 0;
    if (b0 >= 32 && b0 <= 246) args.push(b0 - 139);
    else if (b0 >= 247 && b0 <= 250) args.push((b0 - 247) * 256 + (b[i++] ?? 0) + 108);
    else if (b0 >= 251 && b0 <= 254) args.push(-(b0 - 251) * 256 - (b[i++] ?? 0) - 108);
    else if (b0 === 28) {
      args.push((((b[i] ?? 0) << 8) | (b[i + 1] ?? 0)) << 16 >> 16);
      i += 2;
    } else if (b0 === 29) {
      args.push(
        ((b[i] ?? 0) << 24) | ((b[i + 1] ?? 0) << 16) | ((b[i + 2] ?? 0) << 8) | (b[i + 3] ?? 0),
      );
      i += 4;
    } else if (b0 === 30) {
      // 實數：用不到，讀掉
      for (;;) {
        const n = b[i++] ?? 0xff;
        if ((n & 15) === 15 || n >> 4 === 15) break;
      }
      args.push(0);
    } else {
      const op = b0 === 12 ? 1200 + (b[i++] ?? 0) : b0;
      out.set(op, args);
      args = [];
    }
  }
  return out;
}

function fdSelect(b: Uint8Array, at: number, glyphs: number): (g: number) => number {
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const fmt = b[at] ?? 0;
  if (fmt === 0) return (g) => b[at + 1 + g] ?? 0;
  const ranges: Array<[number, number]> = [];
  if (fmt === 3) {
    const n = dv.getUint16(at + 1);
    for (let k = 0; k < n; k++) ranges.push([dv.getUint16(at + 3 + k * 3), b[at + 5 + k * 3] ?? 0]);
  } else if (fmt === 4) {
    const n = dv.getUint32(at + 1);
    for (let k = 0; k < n; k++) ranges.push([dv.getUint32(at + 5 + k * 6), dv.getUint16(at + 9 + k * 6)]);
  } else throw new Error(`FDSelect format ${fmt}`);
  return (g) => {
    let fd = 0;
    for (const [first, id] of ranges) {
      if (first > g || first >= glyphs) break;
      fd = id;
    }
    return fd;
  };
}

/** 每個 ItemVariationData 有幾個變化區域（`blend` 要知道每個值後面跟了幾個差量）。 */
function regionCounts(b: Uint8Array, at: number): number[] {
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const store = at + 2; // 前面是 uint16 的長度
  const n = dv.getUint16(store + 6);
  const out: number[] = [];
  for (let k = 0; k < n; k++) {
    const off = dv.getUint32(store + 8 + k * 4);
    out.push(dv.getUint16(store + off + 4));
  }
  return out;
}

// ---------- 筆畫指令 ----------

const MAX_OUT_ARGS = 48; // CFF1 的引數堆疊上限（Noto Sans CJK 全部 65,535 個字形最多用到 46）

function encodeNum(out: number[], v: number) {
  if (Number.isInteger(v)) {
    if (v >= -107 && v <= 107) out.push(v + 139);
    else if (v >= 108 && v <= 1131) out.push(247 + ((v - 108) >> 8), (v - 108) & 255);
    else if (v >= -1131 && v <= -108) out.push(251 + ((-v - 108) >> 8), (-v - 108) & 255);
    else if (v >= -32768 && v <= 32767) out.push(28, (v >> 8) & 255, v & 255);
    else throw new Error("charstring number out of range");
    return;
  }
  const f = Math.round(v * 65536);
  out.push(255, (f >> 24) & 255, (f >> 16) & 255, (f >> 8) & 255, f & 255);
}

const bias = (n: number) => (n < 1240 ? 107 : n < 33900 ? 1131 : 32768);

interface GlyphCtx {
  gsubrs: Index;
  lsubrs: Index | null;
  regions: number[];
  vsindex: number;
  data: Uint8Array;
}

interface Walk {
  out: number[];
  stack: number[];
  stems: number;
  maxArgs: number;
}

function walk(cs: Uint8Array, ctx: GlyphCtx, w: Walk, depth: number) {
  if (depth > 10) throw new Error("subroutine nesting too deep");
  let i = 0;
  const emit = (op: number, op2?: number) => {
    w.maxArgs = Math.max(w.maxArgs, w.stack.length);
    if (w.stack.length > MAX_OUT_ARGS) throw new Error(`CFF2: a charstring operator has ${w.stack.length} operands (CFF1 allows ${MAX_OUT_ARGS})`);
    for (const v of w.stack) encodeNum(w.out, v);
    w.out.push(op);
    if (op2 !== undefined) w.out.push(op2);
    w.stack = [];
  };
  while (i < cs.length) {
    const b0 = cs[i++] ?? 0;
    if (b0 >= 32 || b0 === 28) {
      if (b0 === 28) {
        w.stack.push((((cs[i] ?? 0) << 8) | (cs[i + 1] ?? 0)) << 16 >> 16);
        i += 2;
      } else if (b0 <= 246) w.stack.push(b0 - 139);
      else if (b0 <= 250) w.stack.push((b0 - 247) * 256 + (cs[i++] ?? 0) + 108);
      else if (b0 <= 254) w.stack.push(-(b0 - 251) * 256 - (cs[i++] ?? 0) - 108);
      else if (b0 === 255) {
        const v = ((cs[i] ?? 0) << 24) | ((cs[i + 1] ?? 0) << 16) | ((cs[i + 2] ?? 0) << 8) | (cs[i + 3] ?? 0);
        w.stack.push(v / 65536);
        i += 4;
      }
      continue;
    }
    switch (b0) {
      case 1:
      case 3:
      case 18:
      case 23:
        // 提示：數量要記（hintmask 的遮罩長度要用），指令丟掉
        w.stems += w.stack.length >> 1;
        w.stack = [];
        break;
      case 19:
      case 20:
        w.stems += w.stack.length >> 1; // 前面剩的引數是隱含的 vstem
        w.stack = [];
        i += (w.stems + 7) >> 3;
        break;
      case 10:
      case 29: {
        const subrs = b0 === 10 ? ctx.lsubrs : ctx.gsubrs;
        const idx = (w.stack.pop() ?? 0) + bias(subrs?.count ?? 0);
        if (!subrs || idx < 0 || idx >= subrs.count) throw new Error("bad subroutine index");
        const [a, z] = subrs.at(idx);
        walk(ctx.data.subarray(a, z), ctx, w, depth + 1);
        break;
      }
      case 11:
        return;
      case 15:
        ctx.vsindex = w.stack.pop() ?? 0;
        break;
      case 16: {
        const n = w.stack.pop() ?? 0;
        const k = ctx.regions[ctx.vsindex] ?? 0;
        const base = w.stack.length - n * (k + 1);
        if (base < 0) throw new Error("blend: not enough operands");
        w.stack.splice(base + n, n * k); // 預設值留著，差量丟掉
        break;
      }
      case 12:
        emit(12, cs[i++] ?? 0);
        break;
      case 14:
        // CFF2 沒有 endchar；真的遇到就當結束
        return;
      default:
        emit(b0);
    }
  }
}

// ---------- 寫 CFF1 ----------

function indexBytes(items: Uint8Array[]): number[] {
  const out: number[] = [];
  out.push((items.length >> 8) & 255, items.length & 255);
  if (items.length === 0) return out;
  let total = 1;
  const offs = [1];
  for (const it of items) {
    total += it.length;
    offs.push(total);
  }
  const offSize = total < 256 ? 1 : total < 65536 ? 2 : total < 16777216 ? 3 : 4;
  out.push(offSize);
  for (const o of offs) for (let k = offSize - 1; k >= 0; k--) out.push((o >> (8 * k)) & 255);
  for (const it of items) for (const x of it) out.push(x);
  return out;
}

/** DICT 的整數一律用五位元組（偏移量要先預留固定的寬度）。 */
const dictInt = (v: number) => [29, (v >> 24) & 255, (v >> 16) & 255, (v >> 8) & 255, v & 255];
const STRINGS = ["Adobe", "Identity"]; // SID 391、392

function buildCff1(charstrings: Uint8Array[]): Uint8Array {
  const n = charstrings.length;
  const header = [1, 0, 4, 4];
  const nameIndex = indexBytes([new TextEncoder().encode("Subset")]);
  const strIndex = indexBytes(STRINGS.map((s) => new TextEncoder().encode(s)));
  const gsubrIndex = indexBytes([]);
  const csIndex = indexBytes(charstrings);
  // 字集：CID 與字形編號相同（格式 2，一個範圍）
  const charset = n > 1 ? [2, 0, 1, ((n - 2) >> 8) & 255, (n - 2) & 255] : [2, 0, 1, 0, 0];
  const fdSel = [3, 0, 1, 0, 0, 0, (n >> 8) & 255, n & 255]; // 格式 3：一個範圍、全用 FD 0
  const priv: number[] = []; // 空的 Private DICT
  // Top DICT 長度固定：ROS(3 個 SID/Supplement) + CIDCount + charset + FDSelect + CharStrings + FDArray
  const topBody = (o: { charset: number; fdSel: number; cs: number; fdArr: number }) => [
    ...dictInt(391),
    ...dictInt(392),
    ...dictInt(0),
    12,
    30, // ROS
    ...dictInt(n),
    12,
    34, // CIDCount
    ...dictInt(o.charset),
    15,
    ...dictInt(o.fdSel),
    12,
    37,
    ...dictInt(o.cs),
    17,
    ...dictInt(o.fdArr),
    12,
    36,
  ];
  const topLen = indexBytes([new Uint8Array(topBody({ charset: 0, fdSel: 0, cs: 0, fdArr: 0 }).length)]).length;
  const base = header.length + nameIndex.length + topLen + strIndex.length + gsubrIndex.length;
  const csAt = base;
  const charsetAt = csAt + csIndex.length;
  const fdSelAt = charsetAt + charset.length;
  const privAt = fdSelAt + fdSel.length;
  const fdArrAt = privAt + priv.length;
  const top = indexBytes([
    new Uint8Array(topBody({ charset: charsetAt, fdSel: fdSelAt, cs: csAt, fdArr: fdArrAt })),
  ]);
  const fontDict = new Uint8Array([...dictInt(priv.length), ...dictInt(privAt), 18]);
  const fdArray2 = indexBytes([fontDict]);
  return new Uint8Array([
    ...header,
    ...nameIndex,
    ...top,
    ...strIndex,
    ...gsubrIndex,
    ...csIndex,
    ...charset,
    ...fdSel,
    ...priv,
    ...fdArray2,
  ]);
}

/**
 * 從 `CFF2` 表取出 `gids`（原字形編號）組成 CFF1 子集。新字形的編號：0 是 `.notdef`，
 * 之後依 `gids` 的順序是 1、2、3…（`gids` 裡不要放 0）。
 */
export function cff2ToCff1Subset(cff2: Uint8Array, gids: number[]): Cff1Subset {
  const dv = new DataView(cff2.buffer, cff2.byteOffset, cff2.byteLength);
  if (cff2[0] !== 2) throw new Error("not a CFF2 table");
  const hdr = cff2[2] ?? 5;
  const topLen = dv.getUint16(3);
  const top = parseDict(cff2, hdr, hdr + topLen);
  const gsubrs = readIndex2(cff2, hdr + topLen);
  const csOff = top.get(17)?.[0];
  const fdaOff = top.get(1236)?.[0];
  const fdsOff = top.get(1237)?.[0];
  const vsOff = top.get(24)?.[0];
  if (csOff === undefined || fdaOff === undefined) throw new Error("CFF2: missing CharStrings or FDArray");
  const charstrings = readIndex2(cff2, csOff);
  const fda = readIndex2(cff2, fdaOff);
  const select = fdsOff === undefined ? () => 0 : fdSelect(cff2, fdsOff, charstrings.count);
  const regions = vsOff === undefined ? [] : regionCounts(cff2, vsOff);
  // 每個 FD 的 local subrs 與預設的 vsindex
  const fds = Array.from({ length: fda.count }, (_, k) => {
    const [a, z] = fda.at(k);
    const priv = parseDict(cff2, a, z).get(18);
    if (!priv) return { lsubrs: null as Index | null, vsindex: 0 };
    const [size, off] = priv as [number, number];
    const pd = parseDict(cff2, off, off + size);
    const subrs = pd.get(19)?.[0];
    return { lsubrs: subrs === undefined ? null : readIndex2(cff2, off + subrs), vsindex: pd.get(22)?.[0] ?? 0 };
  });
  const out: Uint8Array[] = [new Uint8Array([14])]; // .notdef：空字形
  let maxArgs = 0;
  for (const gid of gids) {
    if (gid <= 0 || gid >= charstrings.count) {
      out.push(new Uint8Array([14]));
      continue;
    }
    const fd = fds[select(gid)] ?? fds[0];
    const [a, z] = charstrings.at(gid);
    const ctx: GlyphCtx = {
      gsubrs,
      lsubrs: fd?.lsubrs ?? null,
      regions,
      vsindex: fd?.vsindex ?? 0,
      data: cff2,
    };
    const w: Walk = { out: [], stack: [], stems: 0, maxArgs: 0 };
    walk(cff2.subarray(a, z), ctx, w, 0);
    w.out.push(14); // endchar
    maxArgs = Math.max(maxArgs, w.maxArgs);
    out.push(new Uint8Array(w.out));
  }
  return { cff: buildCff1(out), glyphCount: out.length, maxArgs };
}
