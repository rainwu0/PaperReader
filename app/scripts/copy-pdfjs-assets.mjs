// 把 PDF.js 執行時要讀的資源（字型、字元對照表、wasm、色彩描述檔）從 node_modules 複製到 public/pdfjs。
// 這些檔案是 PDF.js（Apache-2.0）的原樣複本，不放進版本庫，建置前由這支腳本產生。
// 例外是 Liberation Sans 的四個字型檔與它的授權檔：pdf.js 內附的是 1.x（GPL-2.0 加字型例外），
// 複製完用 assets/fonts/liberation-sans/ 的官方 2.x（SIL OFL 1.1，原樣不改）蓋過同名檔。
// 不用 fs.cpSync：在這台電腦的 Node 25.7 上它會直接當掉（結束碼 0xC0000409），改成自己遞迴複製。
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

function copyDir(src, dst) {
  mkdirSync(dst, { recursive: true });
  for (const name of readdirSync(src)) {
    const s = resolve(src, name);
    const d = resolve(dst, name);
    if (statSync(s).isDirectory()) copyDir(s, d);
    else copyFileSync(s, d);
  }
}

/** Liberation Sans 2.x 蓋過 pdf.js 內附的 1.x。兩邊的檔都要在：pdf.js 改了檔名時建置失敗，不會變成多放一份沒人用的字型。 */
function replaceLiberation(root, fonts) {
  const own = resolve(root, "assets/fonts/liberation-sans");
  const swaps = [
    ...["Regular", "Bold", "Italic", "BoldItalic"].map((s) => [`LiberationSans-${s}.ttf`, `LiberationSans-${s}.ttf`]),
    ["LICENSE", "LICENSE_LIBERATION"],
  ];
  for (const [from, to] of swaps) {
    const s = resolve(own, from);
    const d = resolve(fonts, to);
    if (!existsSync(s)) throw new Error(`找不到 ${s}（Liberation Sans 2.x）`);
    if (!existsSync(d)) throw new Error(`pdf.js 的 standard_fonts 裡沒有 ${to}：pdf.js 換了字型檔名，先確認它現在用哪些檔`);
    copyFileSync(s, d);
  }
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const from = resolve(root, "node_modules/pdfjs-dist");
const to = resolve(root, "public/pdfjs");
rmSync(to, { recursive: true, force: true });
for (const d of ["standard_fonts", "cmaps", "wasm", "iccs"]) {
  copyDir(resolve(from, d), resolve(to, d));
}
replaceLiberation(root, resolve(to, "standard_fonts"));
console.log("pdfjs 資源已複製到 public/pdfjs（Liberation Sans 換成 2.x）");
