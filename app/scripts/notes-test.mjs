// 筆記窗格撞上別處修改時「哪一段沒存進去」（`src/screens/reader/notesConflict.ts`）的測試。
// 執行：node --test scripts/notes-test.mjs（在 app 底下）

import assert from "node:assert/strict";
import { test } from "node:test";
import { appendPart, unsavedPart } from "../src/screens/reader/notesConflict.ts";

test("沒改過：沒有要留的", () => {
  assert.equal(unsavedPart("想法", "想法"), "");
});

test("在最後接著打：留下新打的那段", () => {
  assert.equal(unsavedPart("第一段。", "第一段。\n\n第二段要記的事。"), "第二段要記的事。");
});

test("在中間插入：只留插入的字", () => {
  assert.equal(unsavedPart("開頭。結尾。", "開頭。中間補一句。結尾。"), "中間補一句。");
});

test("改了英文單字的一部分：補到整個字", () => {
  assert.equal(unsavedPart("I like cats a lot", "I like dogs a lot"), "dogs");
});

test("改了好幾處：從第一處到最後一處", () => {
  assert.equal(unsavedPart("甲乙丙丁戊", "甲X乙丙丁Y戊"), "X乙丙丁Y");
});

test("只刪了字：沒有要留的", () => {
  assert.equal(unsavedPart("一二三四", "一四"), "");
});

test("原本是空的：整段都留", () => {
  assert.equal(unsavedPart("", "全新的想法"), "全新的想法");
});

test("加到最後：中間空一行；原本是空的就直接放", () => {
  assert.equal(appendPart("別處寫的。\n", "我剛才寫的。"), "別處寫的。\n\n我剛才寫的。");
  assert.equal(appendPart("", "我剛才寫的。"), "我剛才寫的。");
});
