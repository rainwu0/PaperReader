// 筆刷的「復原」：這次打開筆刷以來做過的動作，按一下撤銷目前這一頁上的最後一個。
// 畫的 → 刪掉那一筆；擦的 → 把那一筆畫回去；清除一頁 → 整頁畫回去。畫回去的筆跡代號是新的，
// 清單裡還指著舊代號的動作要改指新的（例如「畫了 A、擦了 A」，撤銷擦掉之後再撤銷畫，要刪的是畫回去的那一筆）。
// 只管清單；寫檔由 `useInk.ts` 經核心做。

export interface StrokeLike {
  id: string;
  material: string;
  page: number;
}

export type InkAction<S extends StrokeLike> =
  | { kind: "draw"; material: string; page: number; id: string }
  | { kind: "erase"; material: string; page: number; stroke: S }
  | { kind: "clear"; material: string; page: number; strokes: S[] };

/** 這份材料、這幾頁（畫面上看得到的，頁序從 0 起）上最後一個動作在清單裡的位置；沒有是 -1。 */
export function lastOnPage<S extends StrokeLike>(list: readonly InkAction<S>[], material: string, pages: readonly number[]): number {
  for (let i = list.length - 1; i >= 0; i--) {
    const a = list[i];
    if (a && a.material === material && pages.includes(a.page)) return i;
  }
  return -1;
}

/** 畫回去之後：清單裡指著舊代號的換成新的。沒有改到的動作還是原來那個物件（提示上的「復原」靠它找）。 */
export function remapIds<S extends StrokeLike>(list: readonly InkAction<S>[], ids: ReadonlyMap<string, string>): InkAction<S>[] {
  const swap = (s: S): S => {
    const to = ids.get(s.id);
    return to ? { ...s, id: to } : s;
  };
  return list.map((a) => {
    if (a.kind === "draw") {
      const to = ids.get(a.id);
      return to ? { ...a, id: to } : a;
    }
    if (a.kind === "erase") return ids.has(a.stroke.id) ? { ...a, stroke: swap(a.stroke) } : a;
    return a.strokes.some((s) => ids.has(s.id)) ? { ...a, strokes: a.strokes.map(swap) } : a;
  });
}
