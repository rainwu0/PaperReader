// 匯入同步包的預覽整理（`src/screens/settings/syncModel.ts`）與另一台的閱讀位置（`src/screens/reader/otherDevice.ts`）的測試。
// 預覽的資料是用 `prcli sync-import --dry-run` 實際跑出來的輸出（兩個合成的文獻庫，代號是測試用的固定值），
// 確認畫面讀的欄位名稱（snake_case、大寫開頭的列舉）和核心交來的一樣。
// 執行：node --test scripts/sync-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  choicesToSend,
  defaultChoices,
  groupNotices,
  isEmptyPreview,
  sideOptions,
  totals,
  visibleNotices,
} from "../src/screens/settings/syncModel.ts";
import { otherDevicePosition } from "../src/screens/reader/otherDevice.ts";

// prcli sync-import a3.zip --dry-run 的輸出（A 加了一篇、劃了一條線，兩邊都改了同一篇的想法）
const preview = JSON.parse(`{"preview_id":"01M7G1FHKEZF77SSVH3E2S8PVZ","package":{"package":"01M7G1FH9A8FBP5PCZFV1CDVZ2","created":"2026-11-15T16:43:36.741Z","device":"01JSYNCA00000000000000000A","device_label":"Pixel A","mode":"full","already_imported":false},"added":[{"kind":"Paper","paper":"01M7G1FGS9X92TRXCV06GTH7XC","title":"Sample Three","count":1},{"kind":"Highlight","paper":"01M7G1F03464BSC55060G360QS","title":"Title from B","count":1}],"updated":[],"removed":[],"conflicts":[{"id":"c1","kind":"Thoughts","paper":"01M7G1F03464BSC55060G360QS","title":"Title from B","item":null,"local":{"exists":true,"modified":null,"device":"01JSYNCB00000000000000000B","text":"B changed it"},"incoming":{"exists":true,"modified":null,"device":"01JSYNCA00000000000000000A","text":"A changed it"},"options":["Local","Incoming","Both"],"default":"Both","count":1}],"notices":[{"kind":"ConflictCopy","paper":"01M7G1F03464BSC55060G360QS","path":"notes/Sample One (conflict 01JSYN 20261115T164337Z).md","other":null}],"skipped_papers":[]}`);

test("實際的預覽：欄位名稱與畫面讀的一致", () => {
  assert.equal(preview.package.device_label, "Pixel A");
  assert.equal(preview.package.already_imported, false);
  assert.equal(preview.conflicts[0].default, "Both");
  assert.deepEqual(preview.skipped_papers, []);
  assert.equal(isEmptyPreview(preview), false);
});

test("彙總依種類加總，文獻排在劃線前面", () => {
  assert.deepEqual(totals(preview.added), [
    { kind: "Paper", count: 1 },
    { kind: "Highlight", count: 1 },
  ]);
  const items = [
    { kind: "Highlight", paper: "a", title: "A", count: 2 },
    { kind: "Paper", paper: "b", title: "B", count: 1 },
    { kind: "Highlight", paper: "b", title: "B", count: 3 },
    { kind: "Stroke", paper: "b", title: "B", count: 0 },
    { kind: "SomethingNew", paper: "b", title: "B", count: 1 },
  ];
  assert.deepEqual(totals(items), [
    { kind: "Paper", count: 1 },
    { kind: "Highlight", count: 5 },
    { kind: "SomethingNew", count: 1 },
  ]);
});

test("預設已選：每個衝突用核心給的預設", () => {
  assert.deepEqual(defaultChoices(preview.conflicts), { c1: "Both" });
});

test("送出的選擇：一個不少、不多送，選了不提供的選項換回預設", () => {
  const conflicts = [
    { ...preview.conflicts[0], id: "c1" },
    { ...preview.conflicts[0], id: "c2", kind: "Bib", options: ["Local", "Incoming"], default: "Incoming" },
  ];
  assert.deepEqual(choicesToSend(conflicts, {}), [
    ["c1", "Both"],
    ["c2", "Incoming"],
  ]);
  assert.deepEqual(choicesToSend(conflicts, { c1: "Local", c2: "Both", c9: "Local" }), [
    ["c1", "Local"],
    ["c2", "Incoming"],
  ]);
});

test("兩邊並排的卡片與「兩個都留」依選項出現", () => {
  assert.deepEqual(sideOptions(preview.conflicts[0]), { local: true, incoming: true, both: true });
  assert.deepEqual(sideOptions({ ...preview.conflicts[0], options: ["Local", "Incoming"] }), {
    local: true,
    incoming: true,
    both: false,
  });
});

test("提醒依種類分組，路徑不重複", () => {
  const notices = [
    ...preview.notices,
    { kind: "UnmergedWrite", paper: null, path: "notes/x.md", other: null },
    { kind: "UnmergedWrite", paper: null, path: "notes/x.md", other: null },
    { kind: "UnmergedWrite", paper: "p", path: "papers/y/paper.json", other: null },
  ];
  assert.deepEqual(groupNotices(notices), [
    { kind: "ConflictCopy", key: "ConflictCopy", count: 1, paths: ["notes/Sample One (conflict 01JSYN 20261115T164337Z).md"] },
    { kind: "UnmergedWrite", key: "UnmergedWrite", count: 3, paths: ["notes/x.md", "papers/y/paper.json"] },
  ]);
});

test("合起來太大：參考文獻清單與抽出的文字分成兩句（看路徑）", () => {
  const notices = [
    { kind: "TooLargeToMerge", paper: "a", path: null, other: null },
    { kind: "TooLargeToMerge", paper: "b", path: "papers/B/text.json", other: null },
    { kind: "TooLargeToMerge", paper: "c", path: "papers/C/text.json", other: null },
  ];
  const groups = groupNotices(notices);
  assert.deepEqual(
    groups.map((g) => [g.key, g.count]),
    [
      ["TooLargeToMerge", 1],
      ["TooLargeToMerge.text", 2],
    ],
  );
  assert.ok(groups.every((g) => g.kind === "TooLargeToMerge"));
});

test("提醒跟著選擇：標了衝突的，那些衝突都不是「兩個都留」就隱藏；沒標的一定出現", () => {
  const conflicts = [
    { ...preview.conflicts[0], id: "c1" },
    { ...preview.conflicts[0], id: "c2", kind: "HighlightEdited" },
  ];
  const copy = { kind: "ConflictCopy", paper: "p", path: "notes/a (conflict x).md", other: null, conflicts: ["c1", "c2"] };
  const always = { kind: "NotesOutsideText", paper: "p", path: "notes/a.md", other: null, conflicts: [] };
  const old = { kind: "UnmergedWrite", paper: null, path: "notes/x.md", other: null }; // 舊版核心沒有這個欄位
  const all = [copy, always, old];
  assert.deepEqual(visibleNotices(all, conflicts, {}), all);
  assert.deepEqual(visibleNotices(all, conflicts, { c1: "Local" }), all, "c2 仍是兩個都留");
  assert.deepEqual(visibleNotices(all, conflicts, { c1: "Local", c2: "Incoming" }), [always, old]);
  assert.deepEqual(visibleNotices(all, conflicts, { c1: "Both", c2: "Incoming" }), all);
});

test("沒有任何變更的預覽（例如同一份再匯入一次）", () => {
  const again = { ...preview, added: [], updated: [], removed: [], conflicts: [], package: { ...preview.package, already_imported: true } };
  assert.equal(isEmptyPreview(again), true);
});

// ---------- 另一台的閱讀位置 ----------

const ME = "01JSYNCB00000000000000000B";
const OTHER = "01JSYNCA00000000000000000A";
function pos(device, page, t, material = "m1") {
  return { material, page, offset: 0.2, zoom: null, t, device };
}
function progress(position, devicePositions) {
  return {
    status: "Reading",
    read_at: null,
    position,
    device_positions: devicePositions,
    furthest_pages: {},
    last_read_at: position?.t ?? null,
  };
}

test("最新的位置是另一台記的、比這台新：問一次，先停在這台的位置", () => {
  const mine = pos(ME, 3, "2026-11-15T10:00:00.000Z");
  const theirs = pos(OTHER, 12, "2026-11-15T11:00:00.000Z");
  const r = otherDevicePosition(progress(theirs, [mine, theirs]), ME, "m1");
  assert.deepEqual(r, { other: theirs, own: mine });
});

test("這台沒讀過：不問，直接停在另一台的位置（停在第 1 頁的話，離開時記下的第 1 頁會蓋過它）", () => {
  const theirs = pos(OTHER, 12, "2026-11-15T11:00:00.000Z");
  assert.equal(otherDevicePosition(progress(theirs, [theirs]), ME, "m1"), null);
});

test("不問的情況：這台自己記的、在別份材料、不知道這台是誰、沒有位置", () => {
  const mine = pos(ME, 3, "2026-11-15T12:00:00.000Z");
  const theirs = pos(OTHER, 12, "2026-11-15T11:00:00.000Z");
  assert.equal(otherDevicePosition(progress(mine, [mine, theirs]), ME, "m1"), null);
  const elsewhere = pos(OTHER, 12, "2026-11-15T13:00:00.000Z", "m2");
  assert.equal(otherDevicePosition(progress(elsewhere, [mine, elsewhere]), ME, "m1"), null);
  assert.equal(otherDevicePosition(progress(theirs, [theirs]), null, "m1"), null);
  assert.equal(otherDevicePosition(progress(null, []), ME, "m1"), null);
  assert.equal(otherDevicePosition(null, ME, "m1"), null);
});

test("這台只在別份材料上讀過：同樣不問，直接停在另一台的位置", () => {
  const mineOnTranslation = pos(ME, 3, "2026-11-15T10:00:00.000Z", "m2");
  const theirs = pos(OTHER, 12, "2026-11-15T11:00:00.000Z");
  assert.equal(otherDevicePosition(progress(theirs, [mineOnTranslation, theirs]), ME, "m1"), null);
});
