// 浮動工具箱留在畫面裡的算法（只算位置，不碰畫面），`Toolbox.tsx` 用。
//
// 面板的寬高要和它的位置無關（`reader.css` 的 `.tb-float` 寫明 `width: max-content`）：
// 固定定位、寬度自動的元素會照「左緣到畫面右緣」的空間縮放，夾回畫面時每往左挪 4 像素就變寬 4 像素，
// 每次重畫都再挪一次，工具箱就越點越寬（0.1.3.1 修掉的錯）。所以這裡只拿量到的寬高夾位置，同一組輸入再夾一次結果不變。

export interface FloatBox {
  /** 面板現在的左上角（像素）。 */
  left: number;
  top: number;
  /** 面板的寬高（像素，不含縮放動畫的變形：用 `offsetWidth`／`offsetHeight`）。 */
  width: number;
  height: number;
}

export interface FloatArea {
  /** 畫面的寬高。 */
  width: number;
  height: number;
  /** 狀態列與手勢列佔掉的高度。 */
  insetTop: number;
  insetBottom: number;
}

/** 離畫面邊緣至少留多少。 */
export const FLOAT_MARGIN = 4;

/** 把浮動的面板夾回畫面裡：交回新的左上角。面板比畫面大時貼左上。 */
export function clampFloat(box: FloatBox, area: FloatArea): { left: number; top: number } {
  const m = FLOAT_MARGIN;
  const top = area.insetTop + m;
  const bottom = area.height - area.insetBottom;
  return {
    left: Math.min(Math.max(m, area.width - box.width - m), Math.max(m, box.left)),
    top: Math.min(Math.max(top, bottom - box.height - m), Math.max(top, box.top)),
  };
}
