// 檢查找到的問題的白話說法。核心之後會新增問題種類，這個版本不認得的就用一句通用的說法
// （說發生了什麼、資料沒有被改動、可以怎麼做），不顯示空白或程式代號。
// 不依賴任何東西，所以 `scripts/catalog-test.mjs` 可以直接測。

export function problemText(dictionary: Readonly<Record<string, string>>, kind: string): string {
  const known = Object.hasOwn(dictionary, `problem.${kind}`) ? dictionary[`problem.${kind}`] : undefined;
  return known ?? dictionary["problem.unknown"] ?? "";
}
