// 切條目（`entries.ts`）：用公開組與量測組論文參考文獻的真實片段（CC BY 4.0）。
// 執行：npm run test:ui（或 node --test src/ext/links/entries-test.mjs，在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { entryYear, joinText, numberedStyle, splitEntries } from "./entries.ts";

const box = [72, 100, 300, 120];
const seg = (page, source) => ({ page, bbox: box, kind: "reference", size: 9, source });

test("編號式：每個編號一條，續段接上一條，跨頁、跨欄的接起來（17250 第 5 到 6 頁）", () => {
  // 編號照原文是 4 到 6；這裡從 1 起（清單一定從 1 開始）
  const segs = [
    seg(5, "[1] R. Wolf, Quantum key distribution protocols, in Quan-"),
    seg(5, "tum Key Distribution: An Introduction with Exercises (Springer, 2021) pp. 91–116."),
    seg(5, "[2] M. F. Anka, J. A. Mora Rodríguez, D. F. Pinto, L. Q. Galvão, M. A. Dias, and A. B. Tacla, An introduc-"),
    seg(5, "tory review of the theory of continuous-variable quantum key distribution: Fundamentals, protocols, and security,"),
    seg(6, "Brazilian Journal of Physics 56, 72 (2026)."),
    seg(6, "[3] F. Grosshans and P. Grangier, Continuous variable quantum cryptography using coherent states, arXiv preprint quant-ph/0109084 (2001)."),
    seg(6, "[4] C. Weedbrook, S. Pirandola, R. García-Patrón, N. J. Cerf, T. C. Ralph, J. H. Shapiro, and S. Lloyd, Gaus-"),
    seg(6, "sian quantum information, Reviews of modern physics 84, 621 (2012)."),
    seg(6, "[5] C. Weedbrook, A. M. Lance, W. P. Bowen, T. Symul, T. C. Ralph, and P. K. Lam, Quantum cryptography without switching, Physical review letters 93, 170504 (2004)."),
  ];
  const es = splitEntries(segs);
  assert.deepEqual(es.map((e) => e.number), [1, 2, 3, 4, 5]);
  assert.ok(es[3].text.includes("Gaussian quantum information"));
  assert.equal(es[0].text, "R. Wolf, Quantum key distribution protocols, in Quantum Key Distribution: An Introduction with Exercises (Springer, 2021) pp. 91–116.");
  assert.equal(es[0].label, "[1]");
  assert.equal(es[0].year, 2021);
  assert.equal(es[1].page, 5, "條目起頭的那一頁");
  assert.ok(es[1].text.endsWith("Brazilian Journal of Physics 56, 72 (2026)."));
  assert.ok(es[1].text.includes("introductory review"), "斷行的連字號接回");
  assert.equal(es[1].year, 2026);
  assert.equal(es[2].arxiv, null, "舊式編號前面沒有「arXiv:」不認");
});

test("編號式：第一個編號之前的段落（被收進來的註腳）不要", () => {
  const es = splitEntries([
    seg(14, "∗ melgazza@example.org"),
    seg(14, "[1] N. Gisin and R. Thew, Quantum communication, Nature Photonics 1, 165 (2007)."),
    seg(14, "[2] C. H. Bennett and G. Brassard, Quantum cryptography: Public key distribution and coin tossing, arXiv preprint arXiv:2003.06557 (2020)."),
  ]);
  assert.equal(es.length, 2);
  assert.equal(es[1].arxiv, "2003.06557");
  assert.equal(es[1].year, 2020, "arXiv 編號裡的 2003 不算年份");
});

test("編號式：一段裡出現下一個編號時切開（雙欄的條目被切在段落中間）", () => {
  const segs = [
    seg(8, "[1] Kara-Ali Aliev, Artem Sevastopolsky, Maria Kolos, Dmitry"),
    seg(8, "Ulyanov, and Victor Lempitsky. Neural point-based graphics. In European conference on computer vision, pages 696– 712. Springer, 2020. 3"),
    seg(8, "[2] Ali Behrouz, Peilin Zhong, and Vahab Mirrokni. Ti-"),
    seg(8, "tans: Learning to memorize at test time. arXiv preprint arXiv:2501.00663, 2024. 3"),
    seg(8, "[3] Eric R Chan, Connor Z Lin. Efficient geometry-aware 3d generative adversarial networks. In Pro-"),
    seg(8, "ceedings of the IEEE/CVF conference on computer vision"),
    seg(8, "and pattern recognition, pages 16123–16133, 2022. 2 [4] David Charatan, Sizhe Lester Li, Andrea Tagliasacchi, and"),
    seg(8, "Vincent Sitzmann. pixelsplat: 3d gaussian splats from image pairs. In CVPR, 2024. 3 [5] Anpei Chen. Tensorf: Tensorial radiance fields. In ECCV, 2022. 2"),
  ];
  // 開頭有編號的只占一半：靠全文的編號依序是 1、2、3……認出編號式
  assert.equal(numberedStyle(segs), "bracket");
  const es = splitEntries(segs);
  assert.deepEqual(es.map((e) => e.number), [1, 2, 3, 4, 5]);
  assert.equal(es[1].text, "Ali Behrouz, Peilin Zhong, and Vahab Mirrokni. Titans: Learning to memorize at test time. arXiv preprint arXiv:2501.00663, 2024. 3");
  assert.equal(es[2].year, 2022, "頁碼範圍裡的數字不算");
  assert.ok(es[3].text.startsWith("David Charatan"));
  assert.equal(es[3].label, "[4]");
  assert.equal(es[4].text, "Anpei Chen. Tensorf: Tensorial radiance fields. In ECCV, 2022. 2");
});

test("單欄：開頭有編號的段落只有一半也認得（17331）", () => {
  const segs = [];
  for (let n = 1; n <= 8; n++) {
    segs.push(seg(9, `[${n}] Author ${n}, Other Person, and Third Person. A title number ${n} about radiance`));
    segs.push(seg(9, `fields and more words. In CVPR, 20${10 + n}.`));
  }
  const es = splitEntries(segs);
  assert.equal(es.length, 8);
  assert.equal(es[7].year, 2018);
  assert.ok(es[0].text.endsWith("In CVPR, 2011."));
});

test("作者年份式：作者開頭的段落是新的一條，續行接上；一段裡幾條時切開（17159 的 SAGE 樣式）", () => {
  const segs = [
    seg(15, "Agarwal R, Machado MC, Castro PS and Bellemare MG"),
    seg(15, "(2021) Contrastive behavioral similarity embeddings for generalization in reinforcement learning. In: International Conference on Learning Representations."),
    seg(15, "Agresti A (2002) Categorical Data Analysis. Wiley Series in"),
    seg(15, "Probability and Statistics, 2nd edition. Hoboken, NJ: Wiley- Interscience."),
    seg(15, "Arvanitidis G, Hansen LK and Hauberg S (2018) Latent space"),
    seg(15, "oddity: On the curvature of deep generative models. In: International Conference on Learning Representations. Beik-Mohammadi H, Hauberg S, Arvanitidis"),
    seg(15, "G and Rozo L (2021) Learning Riemannian manifolds for geodesic motion skills. In: Robotics: Science and Systems."),
  ];
  assert.equal(numberedStyle(segs), null);
  const es = splitEntries(segs);
  assert.deepEqual(
    es.map((e) => e.text.slice(0, 16)),
    ["Agarwal R, Macha", "Agresti A (2002)", "Arvanitidis G, H", "Beik-Mohammadi H"],
  );
  assert.deepEqual(es.map((e) => e.year), [2021, 2002, 2018, 2021]);
  assert.equal(es[1].text, "Agresti A (2002) Categorical Data Analysis. Wiley Series in Probability and Statistics, 2nd edition. Hoboken, NJ: Wiley- Interscience.");
  assert.equal(es[0].number, null);
});

test("作者年份式：「年份加句點」之後緊接作者名單時切開（2602.21204 的 ICML 樣式）", () => {
  const es = splitEntries([
    seg(10, "Zhang, T., Bi, S., Hong, Y., Zhang, K., Luan, F., Yang, S., Sunkavalli, K., Freeman, W. T., and Tan, H. Test-time training done right. arXiv preprint arXiv:2505.23884, 2025. Zhang, Y. and Yang, S. Flame: Flash language modeling made easy, January 2025."),
  ]);
  assert.equal(es.length, 2);
  assert.equal(es[0].arxiv, "2505.23884");
  assert.equal(es[0].year, 2025);
  assert.ok(es[1].text.startsWith("Zhang, Y. and Yang, S."));
});

test("作者年份式：上一條還沒完（作者名單斷在逗號）時，下一段像書目開頭也接上去（2602.21204）", () => {
  const es = splitEntries([
    seg(9, "Yang, S. and Zhang, Y. Fla: A triton-based library for hardware-efficient implementations of linear attention mechanism, 2024."),
    seg(10, "Zhang, T., Bi, S., Hong, Y., Zhang, K., Luan, F., Yang, S.,"),
    seg(10, "Sunkavalli, K., Freeman, W. T., and Tan, H. Test-time training done right. arXiv preprint arXiv:2505.23884, 2025."),
  ]);
  assert.equal(es.length, 2);
  assert.ok(es[1].text.startsWith("Zhang, T., Bi, S."));
  assert.equal(es[1].arxiv, "2505.23884");
});

test("DOI、CoRR、arXiv 網址；括號裡的年份優先", () => {
  const es = splitEntries([
    seg(3, "[1] A. Author, Some paper, J. Phys. 1, 2 (2019), doi:10.1103/PhysRevLett.123.456789."),
    seg(3, "[2] B. Author. Another paper. CoRR abs/1706.03762, 2017."),
    seg(3, "[3] C. Author. Third. https://arxiv.org/abs/2304.08069v2, 2023."),
  ]);
  assert.equal(es[0].doi, "10.1103/physrevlett.123.456789");
  assert.equal(es[0].year, 2019);
  assert.equal(es[1].arxiv, "1706.03762");
  assert.equal(es[1].year, 2017);
  assert.equal(es[2].arxiv, "2304.08069");
  assert.equal(entryYear("Name (2019b). Title. Journal 12, 2020–2021."), 2019);
  assert.equal(entryYear("J. Plucker. On a new geometry of space. Phil. Trans., 1865."), 1865);
});

test("截斷與上限：一條最多 1000 字、最多 2000 條", () => {
  const long = "[1] " + "word ".repeat(400) + "(2020).";
  const es = splitEntries([seg(0, long), seg(0, "[2] B. Author. Short. 2021.")]);
  assert.equal(Array.from(es[0].text).length, 1000);
  const many = [];
  for (let n = 1; n <= 2100; n++) many.push(seg(0, `[${n}] A. B. Title ${n}. 2020.`));
  assert.equal(splitEntries(many).length, 2000);
});

test("接段：連字號只在字母後面、下一段小寫開頭時接回", () => {
  assert.equal(joinText("Quan-", "tum key"), "Quantum key");
  assert.equal(joinText("Wiley-", "Interscience"), "Wiley- Interscience");
  assert.equal(joinText("pp. 91–", "116."), "pp. 91– 116.");
});
