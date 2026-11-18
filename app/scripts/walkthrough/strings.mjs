// 驗收腳本找畫面元件用的文字：一律用字串鍵從應用的字串表（`src/strings/`）取，不在腳本裡寫死畫面上的字。
// 畫面改了文案，腳本跟著變；字串鍵被改名或刪掉，`T` 會直接報錯，不會默默找不到元件。
//
// 字串表把各區拆成幾個檔，彼此用不帶副檔名的方式引入（打包工具看得懂，Node 看不懂），
// 所以先補一個解析規則：找不到時再試加上 `.ts`（和 `scripts/catalog-test.mjs` 同一個做法）。

import { registerHooks } from "node:module";

registerHooks({
  resolve(specifier, context, nextResolve) {
    try {
      return nextResolve(specifier, context);
    } catch (e) {
      if (e?.code === "ERR_MODULE_NOT_FOUND" && /^\.\.?\//.test(specifier) && !/\.[a-z]+$/i.test(specifier)) {
        return nextResolve(`${specifier}.ts`, context);
      }
      throw e;
    }
  },
});

const { zhTW } = await import("../../src/strings/zh-TW.ts");

/** 依字串鍵取出畫面上的文字；`{name}` 換成 `params` 裡的值（和 `src/strings/index.ts` 的 `t` 相同）。 */
export function T(key, params) {
  if (!Object.hasOwn(zhTW, key)) throw new Error(`字串表裡沒有「${key}」：畫面的文字改了，驗收腳本要跟著改`);
  const text = zhTW[key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ""));
}

/** 範本裡 `{…}` 之前的固定部分（例如「讀到第 {page}／{total} 頁」→「讀到第 」），用來找帶數字的句子。 */
export function prefix(key) {
  return T(key).split("{")[0];
}
