// 翻譯設定的小規則（`src/screens/generate/translateConfig.ts`）：明文 http 的警告、實際送到的主機。
// 執行：node --test scripts/translate-config-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  defaultService,
  isInsecureRemote,
  menuServices,
  mergeConsents,
  missingSetting,
  resumeNoteKey,
  supportsTarget,
} from "../src/screens/generate/translateConfig.ts";

// 真論文驗收：每篇都要重選服務，不合理。面板照上次用的服務直接選好；
// 上次的已經停用或刪掉時才用選單第一個（主要服務）。
test("面板照上次用的服務選好", () => {
  const s = (id, enabled = true) => ({ id, enabled });
  const menu = menuServices({ services: [s("anthropic"), s("bergamot"), s("deepl", false)], primary: "anthropic", fallbacks: [] });
  assert.equal(defaultService(menu, "bergamot", null), "bergamot");
  assert.equal(defaultService(menu, null, null), "anthropic");
  assert.equal(defaultService(menu, "deepl", null), "anthropic", "disabled");
  assert.equal(defaultService(menu, "gone", null), "anthropic", "deleted");
  // 這次面板上已經選了的照舊
  assert.equal(defaultService(menu, "bergamot", "anthropic"), "anthropic");
  assert.equal(defaultService([], null, null), null);
});

// 重點筆記的面板也照上次產生重點筆記用的服務選好；它的選單只有對話模型類，
// 上次的不在這份選單上（例如離線引擎）時用選單第一個
test("重點筆記的面板照它自己上次用的服務選好", () => {
  const chat = [{ id: "anthropic" }, { id: "ollama" }];
  assert.equal(defaultService(chat, "ollama", null), "ollama");
  assert.equal(defaultService(chat, "bergamot", null), "anthropic");
});

// 真論文驗收：離線引擎「接著翻」的確認頁寫「會送出」，其實不送出裝置。說法照引擎申報。
test("接著翻的說法照引擎會不會送出裝置", () => {
  assert.equal(resumeNoteKey({ leavesDevice: false }), "generate.confirm.resumeNote.local");
  assert.equal(resumeNoteKey({ leavesDevice: true }), "generate.confirm.resumeNote");
  // 還不知道引擎時照會送出的說法（比較保守）
  assert.equal(resumeNoteKey(null), "generate.confirm.resumeNote");
});

// 舊的本機紀錄搬進設定檔時和設定檔裡已有的合併，不重複、不丟舊的
test("同意清單合併", () => {
  assert.deepEqual(mergeConsents([], ["a@x", "b@y"]), ["a@x", "b@y"]);
  assert.deepEqual(mergeConsents(["a@x"], ["a@x", "c@z"]), ["a@x", "c@z"]);
  assert.deepEqual(mergeConsents(["a@x"], []), ["a@x"]);
  assert.deepEqual(mergeConsents([], ["", "a@x"]), ["a@x"]);
});

test("http 只有本機、模擬器與私人網段不警告", () => {
  for (const ok of [
    "https://api.example.com/v1/",
    "http://localhost:11434/v1/",
    "http://127.0.0.1:8787/v1/",
    "http://10.0.2.2:8787/v1/",
    "http://192.168.1.5:11434/v1/",
    "http://172.20.0.3/v1/",
    "http://my-pc.local:11434/v1/",
    "",
  ]) {
    assert.equal(isInsecureRemote(ok), false, ok);
  }
  for (const bad of ["http://api.example.com/v1/", "http://8.8.8.8/v1/", "http://172.32.0.1/v1/"]) {
    assert.equal(isInsecureRemote(bad), true, bad);
  }
});

// 引擎申報的樣本（真的清單由外殼 `translate_engines` 交回，這裡只放測試用到的欄位）
const deepl = { provider: "deepl", key: "required", model: "no", endpoint: "optional", defaultModel: "", officialHost: "api.deepl.com" };
const compat = { provider: "openai_compatible", key: "optional", model: "required", endpoint: "required", defaultModel: "", officialHost: "" };
const bergamot = { provider: "bergamot", key: "no", model: "no", endpoint: "no", defaultModel: "", officialHost: "", targets: [{ code: "zh-TW" }] };

test("要填什麼照引擎申報", () => {
  const svc = (o) => ({ model: "", endpoint: "", hasKey: false, ...o });
  assert.equal(missingSetting(svc({}), deepl, true), "key");
  assert.equal(missingSetting(svc({ hasKey: true }), deepl, true), null);
  assert.equal(missingSetting(svc({}), compat, true), "endpoint");
  assert.equal(missingSetting(svc({ endpoint: "http://x/v1" }), compat, true), "model");
  assert.equal(missingSetting(svc({ model: "m", endpoint: "http://x/v1" }), compat, true), null, "a compatible endpoint does not need a key");
  assert.equal(missingSetting(svc({ hasKey: true }), deepl, false), "platform");
  // 不用金鑰的引擎：平台存不了金鑰也沒關係
  assert.equal(missingSetting(svc({}), bergamot, false), null);
  assert.equal(supportsTarget(bergamot, "zh-tw"), true);
  assert.equal(supportsTarget(bergamot, "en"), false);
});

test("選單只列啟用的服務：主要服務在前，接著依備用順序", () => {
  const s = (id, enabled = true) => ({ id, name: "", provider: "anthropic", model: "", endpoint: "", enabled, rpm: 0, hasKey: false, host: "" });
  const list = menuServices({
    services: [s("a"), s("b"), s("c", false), s("d"), s("e")],
    primary: "d",
    fallbacks: ["e", "c"],
  });
  assert.deepEqual(list.map((x) => x.id), ["d", "e", "a", "b"]);
});
