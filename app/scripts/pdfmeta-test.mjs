// 「PDF 自帶的篇名與作者要不要採用」「年份從哪裡取」「DOI 與 arXiv 編號從哪裡取」（`src/pdfMeta.ts`）的測試。
// 執行：node --test scripts/pdfmeta-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  arxivFromStamp,
  arxivFromXmp,
  arxivIdsIn,
  doiCandidates,
  doiFromFirstPage,
  doiFromInfo,
  doiFromXmp,
  pickArxiv,
  pickDoi,
  pickYear,
  trimDoi,
  usableAuthors,
  usableTitle,
  yearFromFirstPage,
  yearFromPdfDate,
} from "../src/pdfMeta.ts";

// ---------- DOI 與 arXiv 編號 ----------

test("DOI：自帶資訊的 doi 鍵（含 PDF.js 放在 Custom 裡的）", () => {
  assert.equal(doiFromInfo({ doi: "10.1103/PhysRevA.111.012345" }), "10.1103/physreva.111.012345");
  assert.equal(doiFromInfo({ Custom: { DOI: "https://doi.org/10.1145/3292500.3330701" } }), "10.1145/3292500.3330701");
  assert.equal(doiFromInfo({ Title: "x" }), null);
  // 兩個不同的候選不取
  assert.equal(doiFromInfo({ doi: "10.1000/a", Custom: { doi: "10.1000/b" } }), null);
});

test("DOI：XMP 的 prism:doi 與 doi: 開頭的 dc:identifier", () => {
  const prism = `<rdf:Description><prism:doi>10.1038/s41586-021-03819-2</prism:doi></rdf:Description>`;
  assert.equal(doiFromXmp(prism), "10.1038/s41586-021-03819-2");
  assert.equal(doiFromXmp(`<rdf:Description prism:doi="10.1016/J.CELL.2020.01.001"/>`), "10.1016/j.cell.2020.01.001");
  const dc = `<dc:identifier><rdf:Bag><rdf:li>doi:10.1093/mnras/stab123</rdf:li></rdf:Bag></dc:identifier>`;
  assert.equal(doiFromXmp(dc), "10.1093/mnras/stab123");
  assert.equal(doiFromXmp(`<dc:identifier>urn:isbn:1234</dc:identifier>`), null);
  assert.equal(doiFromXmp(null), null);
});

test("DOI：第一頁只取有標的；別篇的、兩個候選都不取", () => {
  assert.equal(doiFromFirstPage("Phys. Rev. A 111, 012345 – Published 3 January 2025\nDOI: 10.1103/PhysRevA.111.012345"), "10.1103/physreva.111.012345");
  assert.equal(doiFromFirstPage("https://doi.org/10.1145/3292500.3330701."), "10.1145/3292500.3330701");
  assert.equal(doiFromFirstPage("doi:10.1016/j.cell.2020.01.001;"), "10.1016/j.cell.2020.01.001");
  // IEEE 期刊固定印「Digital Object Identifier」
  assert.equal(doiFromFirstPage("Digital Object Identifier 10.1109/ACCESS.2024.1234567"), "10.1109/access.2024.1234567");
  assert.equal(doiFromFirstPage("Digital Object Identifier: 10.1109/TPAMI.2025.0001."), "10.1109/tpami.2025.0001");
  // 同一個來源兩個候選照舊不取
  assert.equal(doiFromFirstPage("Digital Object Identifier 10.1109/a.1\nDOI: 10.1109/b.2"), null);
  // 只有別篇的（沒有標）
  assert.equal(doiFromFirstPage("as shown in [3] (10.1109/CVPR.2016.90), residual"), null);
  // 標了兩個不同的
  assert.equal(doiFromFirstPage("DOI: 10.1000/a\nRelated: doi.org/10.1000/b"), null);
  // 同一個出現兩次只算一個
  assert.equal(doiFromFirstPage("DOI 10.1000/a ... https://doi.org/10.1000/A"), "10.1000/a");
});

test("DOI 尾端的標點與沒有配對的括號去掉，裡面的括號留著", () => {
  assert.equal(trimDoi("10.1002/(SICI)1097-4636(199601)30:1<1::AID-JBM1>3.0.CO;2-#"), "10.1002/(sici)1097-4636(199601)30:1<1::aid-jbm1>3.0.co;2-#");
  assert.equal(trimDoi("10.1000/abc)."), "10.1000/abc");
  assert.equal(trimDoi("10.1000/abc(1)"), "10.1000/abc(1)");
  assert.equal(trimDoi("10.1000/abc:;,."), "10.1000/abc");
  assert.deepEqual(doiCandidates("(see 10.1000/x), and 10.2000/y."), ["10.1000/x", "10.2000/y"]);
});

test("DOI：依序取自帶資訊、XMP、第一頁", () => {
  const page = "DOI: 10.3000/page";
  assert.equal(pickDoi({ doi: "10.1000/info" }, "<prism:doi>10.2000/xmp</prism:doi>", page), "10.1000/info");
  assert.equal(pickDoi({}, "<prism:doi>10.2000/xmp</prism:doi>", page), "10.2000/xmp");
  assert.equal(pickDoi({}, null, page), "10.3000/page");
  assert.equal(pickDoi({}, null, "nothing here 10.4000/unmarked"), null);
});

test("arXiv：頁邊的戳記（帶版本號與類別）；正文裡的編號不算", () => {
  const stamp = "arXiv:2304.08069v3  [cs.CV]  3 Apr 2024\nDETRs Beat YOLOs on Real-time Object Detection";
  assert.equal(arxivFromStamp(stamp), "2304.08069");
  assert.equal(arxivFromStamp("arXiv:hep-th/9901001v2 [hep-th] 5 Jan 1999"), "hep-th/9901001");
  assert.equal(arxivFromStamp("we build on arXiv:2106.09685 and follow"), null);
  assert.equal(pickArxiv(null, "we build on arXiv:2106.09685 and follow"), null);
  // PDF.js 的文字裡前面常黏著別的字
  assert.equal(arxivFromStamp("2026arXiv:2611.16305v1 [eess.SP] 18 Nov 2026"), "2611.16305");
});

test("arXiv：XMP 裡以 arXiv: 開頭的識別碼；戳記優先", () => {
  const xmp = `<dc:identifier>arXiv:2611.17250v1</dc:identifier>`;
  assert.equal(arxivFromXmp(xmp), "2611.17250");
  assert.equal(arxivFromXmp(`<dc:description>cites arXiv:1706.03762</dc:description>`), null);
  assert.equal(pickArxiv(xmp, "nothing"), "2611.17250");
  assert.equal(pickArxiv(xmp, "arXiv:2304.08069v1 [cs.CV] 1 May 2023"), "2304.08069");
});

test("arXiv：XMP 裡的網址寫法（arXiv 產生的 PDF 就是這樣寫）", () => {
  // 公開組 arxiv_2611.16305 的 XMP 原樣（屬性的寫法）
  const attr = `<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" rdf:about="" dc:identifier="https://arxiv.org/abs/2611.16305v1"/>`;
  assert.equal(arxivFromXmp(attr), "2611.16305");
  assert.equal(arxivFromXmp(`<dc:identifier>http://arxiv.org/abs/hep-th/9901001v2</dc:identifier>`), "hep-th/9901001");
  assert.equal(arxivFromXmp(`<dc:identifier>arxiv.org/abs/2304.08069</dc:identifier>`), "2304.08069");
  // 說明裡順帶提到的網址不算；兩個不同的不取
  assert.equal(arxivFromXmp(`<dc:description>see https://arxiv.org/abs/1706.03762</dc:description>`), null);
  assert.equal(arxivFromXmp(`${attr}<dc:identifier>arXiv:2304.08069</dc:identifier>`), null);
  // 沒有頁邊戳記時，補書目從 XMP 取到
  assert.equal(pickArxiv(attr, "A paper without a stamp"), "2611.16305");
});

test("arXiv：條目裡的 arxiv.org 網址也認", () => {
  assert.deepEqual(arxivIdsIn("C. Author. Third. https://arxiv.org/abs/2304.08069v2, 2023."), ["2304.08069"]);
  assert.deepEqual(arxivIdsIn("http://arxiv.org/pdf/1706.03762.pdf"), ["1706.03762"]);
});

test("arXiv：參考文獻條目裡的編號（外掛用同一組）", () => {
  assert.deepEqual(arxivIdsIn("arXiv preprint arXiv:1706.03762, 2017. CoRR abs/1512.03385; arXiv:1706.03762v5"), [
    "1706.03762",
    "1512.03385",
  ]);
  assert.deepEqual(arxivIdsIn("arXiv:math.GT/0309136 and ARXIV:HEP-TH/9901001"), ["math.GT/0309136"]);
});

const FILE = "Zhao 2024.pdf";

test("正常的篇名會採用，空白整理過", () => {
  assert.equal(usableTitle("  DETRs  Beat YOLOs on Real-time Object Detection ", FILE), "DETRs Beat YOLOs on Real-time Object Detection");
  assert.equal(usableTitle("注意力機制的研究", FILE), "注意力機制的研究");
});

test("垃圾篇名不採用", () => {
  for (const bad of [
    null,
    undefined,
    "",
    "   ",
    "ab",
    "Untitled",
    "untitled document",
    "Untitled-1",
    "無標題",
    "Microsoft Word - report final.doc",
    "Microsoft Word - report",
    "paper_v3.docx",
    "draft.tex",
    "scan0001.pdf",
    "12345",
    "2024-11-15",
    "Document1",
    "x".repeat(401),
  ]) {
    assert.equal(usableTitle(bad, FILE), null, `應該不採用：${JSON.stringify(bad)}`);
  }
});

test("等於檔名本身的篇名不採用（那等於沒有資訊）", () => {
  assert.equal(usableTitle("Zhao 2024", FILE), null);
  assert.equal(usableTitle("zhao 2024.PDF", FILE), null);
  assert.equal(usableTitle("Zhao 2024", "Zhao 2024"), null);
});

test("作者：一位或多位", () => {
  assert.deepEqual(usableAuthors("Yian Zhao"), [{ literal: "Yian Zhao" }]);
  assert.deepEqual(usableAuthors("Zhao, Yian"), [{ literal: "Zhao, Yian" }], "一個逗號多半是「姓, 名」");
  assert.deepEqual(usableAuthors("Yian Zhao; Wenyu Lv"), [{ literal: "Yian Zhao" }, { literal: "Wenyu Lv" }]);
  assert.deepEqual(usableAuthors("Yian Zhao and Wenyu Lv"), [{ literal: "Yian Zhao" }, { literal: "Wenyu Lv" }]);
  assert.deepEqual(usableAuthors("Yian Zhao, Wenyu Lv, Shangliang Xu"), [
    { literal: "Yian Zhao" },
    { literal: "Wenyu Lv" },
    { literal: "Shangliang Xu" },
  ]);
  assert.deepEqual(usableAuthors("王小明、李大華"), [{ literal: "王小明" }, { literal: "李大華" }]);
});

test("垃圾作者不採用", () => {
  for (const bad of [null, undefined, "", " ", "a", "Unknown", "admin", "Windows User", "anonymous", "1234"]) {
    assert.deepEqual(usableAuthors(bad), [], `應該不採用：${JSON.stringify(bad)}`);
  }
});

// ---------- 年份 ----------

const NOW = 2026;

// 公開組（arXiv、CC BY 4.0）第一頁經 PDF.js 讀出的真實文字片段（每行一個換行，照 `pdfInfo.ts` 的接法）。
// 這八篇的 PDF 自帶資訊都沒有日期，年份只能從第一頁取。
const PUBLIC_FIRST_PAGES = [
  ["arxiv_2611.16305", "-\nciently addressed.arXiv:2611.16305v1 [eess.SP] 12 Nov 2026"],
  ["arxiv_2611.16534", "hanged at inference.arXiv:2611.16534v1 [cs.CV] 12 Nov 2026"],
  ["arxiv_2611.16841", "within a single unitarXiv:2611.16841v1 [cond-mat.str-el] 12 Nov 2026"],
  ["arxiv_2611.17159", "Email: benc00@vt.eduarXiv:2611.17159v1 [cs.RO] 12 Nov 2026"],
  [
    "arxiv_2611.17242",
    "Astronomy & Astrophysics manuscript no. aa61972-26 ©ESO 2026\nNovember 13, 2026\nCharacterising a population of fast-declining SNe Ia in galaxy\nticle number, page 1arXiv:2611.17242v1 [astro-ph.CO] 12 Nov 2026",
  ],
  ["arxiv_2611.17250", "quadrature, here de-arXiv:2611.17250v1 [quant-ph] 12 Nov 2026"],
  ["arxiv_2611.17316", "herits the changed\n1arXiv:2611.17316v1 [cs.AI] 12 Nov 2026"],
  ["arxiv_2611.17331", "at NVIDIA.\nPreprint.arXiv:2611.17331v1 [cs.CV] 12 Nov 2026"],
];

test("公開組八篇：第一頁頁邊的 arXiv 戳記讀出 2026", () => {
  for (const [name, text] of PUBLIC_FIRST_PAGES) {
    assert.equal(yearFromFirstPage(text, NOW), 2026, name);
    assert.equal(pickYear(undefined, text, NOW), 2026, name);
  }
});

test("PDF 自帶資訊的建立日期", () => {
  assert.equal(yearFromPdfDate("D:20230415120000+08'00'", NOW), 2023);
  assert.equal(yearFromPdfDate("D:2019", NOW), 2019);
  assert.equal(yearFromPdfDate("20180102", NOW), 2018);
  for (const bad of [undefined, null, 20230415, "", "D:", "yesterday", "D:18990101", "D:20990101"]) {
    assert.equal(yearFromPdfDate(bad, NOW), null, `應該不採用：${JSON.stringify(bad)}`);
  }
});

test("第一頁優先，最後才用建立日期", () => {
  const page = "within a single unitarXiv:2611.16841v1 [cond-mat.str-el] 12 Nov 2026";
  // 改版重編過的 arXiv PDF：建立日期是重編那年，頁邊的戳記才是這篇的年份
  assert.equal(pickYear("D:20250101000000Z", page, NOW), 2026);
  assert.equal(pickYear("garbage", page, NOW), 2026);
  // 第一頁找不到年份：才用建立日期
  assert.equal(pickYear("D:20190101000000Z", "An abstract without any year.", NOW), 2019);
  assert.equal(pickYear("D:20190101000000Z", "", NOW), 2019);
  assert.equal(pickYear(undefined, "", NOW), null);
});

test("已出版的論文：版權年份排在第一頁引用的別篇 arXiv 編號之前", () => {
  const page = "Our code builds on arXiv:1706.03762.\n0018-9545 © 2019 IEEE. Personal use is permitted";
  assert.equal(yearFromFirstPage(page, NOW), 2019);
  assert.equal(pickYear("D:20240101000000Z", page, NOW), 2019);
  // 會議也一樣
  assert.equal(yearFromFirstPage("see arXiv:1706.03762\nPublished as a conference paper at ICLR 2024", NOW), 2024);
});

test("arXiv 編號：新式、舊式、第一頁引用的別篇不搶過頁邊的戳記", () => {
  assert.equal(yearFromFirstPage("see arXiv:2304.08069", NOW), 2023);
  assert.equal(yearFromFirstPage("arXiv:0706.1234v2 [hep-th] 9 Jun 2007", NOW), 2007);
  assert.equal(yearFromFirstPage("arXiv:hep-th/9901001v3 [hep-th] 4 Feb 1999", NOW), 1999);
  assert.equal(yearFromFirstPage("arXiv:math.GT/0309136", NOW), 2003);
  // 註腳引用了 2017 年的一篇，頁邊的戳記是這篇自己的
  assert.equal(
    yearFromFirstPage("Code: arXiv:1706.03762.\nfooter\nunitarXiv:2611.16841v1 [cond-mat.str-el] 12 Nov 2026", NOW),
    2026,
  );
  // 不存在的月份、新式編號在 2007 年以前、未來
  assert.equal(yearFromFirstPage("arXiv:2313.00001", NOW), null);
  assert.equal(yearFromFirstPage("arXiv:0612.0001", NOW), null);
  assert.equal(yearFromFirstPage("arXiv:3001.00001", NOW), null);
});

test("版權與會議年份", () => {
  assert.equal(yearFromFirstPage("0018-9545 © 2019 IEEE. Personal use is permitted", NOW), 2019);
  assert.equal(yearFromFirstPage("Copyright © 2021 by the authors.", NOW), 2021);
  assert.equal(yearFromFirstPage("(c) 2015 Elsevier Ltd. All rights reserved.", NOW), 2015);
  assert.equal(yearFromFirstPage("Published as a conference paper at ICLR 2024", NOW), 2024);
  assert.equal(yearFromFirstPage("37th Conference on Neural Information Processing Systems (NeurIPS 2023).", NOW), 2023);
  assert.equal(yearFromFirstPage("Proceedings of the 2021 Conference on Empirical Methods", NOW), 2021);
  assert.equal(yearFromFirstPage("Accepted to ICRA 2022", NOW), 2022);
  // 版權在會議之前
  assert.equal(yearFromFirstPage("CVPR 2020\n© 2021 IEEE", NOW), 2021);
});

test("年份抓錯的情形不採用", () => {
  for (const bad of [
    "This problem has received much attention since 2017.",
    "Downloaded on March 03,2024 at 10:15:22 UTC from IEEE Xplore.",
    "Code: https://www.example.com/2019/code",
    "Chi-square test (Pearson, 1900)",
    "© ESO\n2026", // 年份不在同一行
    "Conference on Things 2099",
  ]) {
    assert.equal(yearFromFirstPage(bad, NOW), null, `應該不採用：${JSON.stringify(bad)}`);
  }
});
