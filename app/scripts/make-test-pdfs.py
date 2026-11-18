"""產生模擬器驗收用的合成 PDF（不進版本庫，可以隨時重產）。只用 Python 標準函式庫。

用法：python make-test-pdfs.py <輸出資料夾>
walkthrough.ps1 與 emulator-check.ps1 在 app\\target\\test-pdfs 沒有這三份時自己呼叫它。

產生三份：
  合成-30頁.pdf   雙欄文字，30 頁，每頁有可搜尋的編號文字（第一頁出現時間、跨欄選取）
  合成-5MB.pdf    約 5MB（寫入、讀回、比對位元組數）
  合成-大檔.pdf   50MB 以上、500 頁以上（大檔、縮放、捲完不閃退）
每頁文字含「Page N」與「P{N}L{M}」，方便在手機上確認停在第幾頁、選到哪幾行。
"""
import random
import sys
import os

WORDS = ("alpha beta gamma delta signal model network layer feature object detect "
         "transformer attention encoder decoder token vector loss gradient sample "
         "baseline dataset accuracy recall precision benchmark latency memory").split()


class Pdf:
    def __init__(self):
        self.objs = []  # list of bytes (object body without "n 0 obj" wrapper)

    def add(self, body: bytes) -> int:
        self.objs.append(body)
        return len(self.objs)  # object number (1-based)

    def reserve(self) -> int:
        self.objs.append(None)
        return len(self.objs)

    def set(self, num: int, body: bytes):
        self.objs[num - 1] = body

    def save(self, path: str, root: int):
        out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
        offsets = []
        for i, body in enumerate(self.objs, start=1):
            offsets.append(len(out))
            out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
        xref = len(out)
        out += b"xref\n0 %d\n" % (len(self.objs) + 1)
        out += b"0000000000 65535 f \n"
        for off in offsets:
            out += b"%010d 00000 n \n" % off
        out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
            len(self.objs) + 1, root, xref)
        with open(path, "wb") as f:
            f.write(out)


def stream(data: bytes, extra: bytes = b"") -> bytes:
    return b"<< /Length %d %s>>\nstream\n" % (len(data), extra) + data + b"\nendstream"


def text_block(rng, page_no, x, y_top, lines, col_tag):
    ops = [b"BT /F1 9 Tf 11 TL %d %d Td" % (x, y_top)]
    for m in range(lines):
        words = " ".join(rng.choice(WORDS) for _ in range(7))
        ops.append(b"(P%dL%d%s %s) Tj T*" % (page_no, m + 1, col_tag.encode(), words.encode()))
    ops.append(b"ET")
    return b"\n".join(ops)


def build(path, pages, image_side, segments, title):
    rng = random.Random(42)
    pdf = Pdf()
    catalog = pdf.reserve()
    pages_obj = pdf.reserve()
    font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    # 影像資料：先備幾份，頁面輪流用，但每頁都是自己的物件（檔案才會夠大）
    pool = [bytes(rng.getrandbits(8) for _ in range(image_side * image_side)) for _ in range(4)] if image_side else []
    kids = []
    for p in range(1, pages + 1):
        img_ref = None
        if image_side:
            data = bytearray(pool[p % len(pool)])
            data[0] = p % 256  # 讓每頁影像不完全相同
            img_ref = pdf.add(stream(bytes(data),
                b"/Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceGray /BitsPerComponent 8 " % (image_side, image_side)))
        c = []
        c.append(b"BT /F1 14 Tf 50 750 Td (%s - Page %d) Tj ET" % (title.encode(), p))
        c.append(text_block(rng, p, 50, 720, 52, "a"))
        c.append(text_block(rng, p, 320, 720, 52, "b"))
        if img_ref:
            c.append(b"q 200 0 0 150 50 100 cm /Im1 Do Q")
        # 向量圖形：隨機折線
        if segments:
            x, y = 300.0, 160.0
            seg = [b"0.3 w 0.2 0.2 0.8 RG %.1f %.1f m" % (x, y)]
            for _ in range(segments):
                x = min(560, max(280, x + rng.uniform(-6, 6)))
                y = min(190, max(60, y + rng.uniform(-6, 6)))
                seg.append(b"%.1f %.1f l" % (x, y))
            seg.append(b"S")
            c.append(b"\n".join(seg))
        content = pdf.add(stream(b"\n".join(c)))
        res = b"<< /Font << /F1 %d 0 R >>" % font
        if img_ref:
            res += b" /XObject << /Im1 %d 0 R >>" % img_ref
        res += b" >>"
        page = pdf.add(b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 595 842] /Resources %s /Contents %d 0 R >>"
                       % (pages_obj, res, content))
        kids.append(page)
    pdf.set(pages_obj, b"<< /Type /Pages /Count %d /Kids [%s] >>" % (
        len(kids), b" ".join(b"%d 0 R" % k for k in kids)))
    pdf.set(catalog, b"<< /Type /Catalog /Pages %d 0 R >>" % pages_obj)
    pdf.save(path, catalog)
    print(path, "pages=%d size=%.1fMB" % (pages, os.path.getsize(path) / 1048576))


if __name__ == "__main__":
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    build(os.path.join(out, "合成-30頁.pdf"), 30, 0, 300, "Synthetic Paper (30 pages)")
    build(os.path.join(out, "合成-5MB.pdf"), 70, 256, 200, "Synthetic 5MB")
    build(os.path.join(out, "合成-大檔.pdf"), 620, 256, 1200, "Synthetic Large (620 pages)")
