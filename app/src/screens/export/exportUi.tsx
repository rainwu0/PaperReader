// 匯出與列印、書目匯出的面板：文獻資訊、閱讀畫面、首頁（群組選單、多選）共用。
// 流程：選一項 → 在暫存資料夾準備好一份檔案 → 交出面板（`ui/DeliverSheet.tsx`）：分享、存到…、列印（PDF 才有）。
// 面板是導覽堆疊的一層，由擁有它的畫面決定何時畫出來；這裡的元件只管面板裡的內容，關閉時呼叫 `onClose`。

import { Fragment, useContext, useEffect, useState, type ReactNode } from "react";
import {
  api,
  kindOf,
  AppError,
  type BibFormat,
  type CitationStyle,
  type ErrorKind,
  type ExportFile,
  type InterchangeFormat,
  type Material,
} from "../../api";
import { errorDetail, errorMessage, t } from "../../strings";
import { FORM_OVERLAY, lastDigest, ROLE_SOURCE } from "../../api/types";
import { BusyOverlay, Button, Choice, ChoiceList, ErrorNotice, KindNotice, Sheet } from "../../ui/components";
import { DeliverSheet } from "../../ui/DeliverSheet";
import { buildAnnotatedPdf } from "./annotatedPdf";
import { NavContext } from "../../nav";

/** 準備好的一份檔案，加上面板要顯示的補充說明。 */
interface Ready {
  file: ExportFile;
  /** 一兩句說明（含幾條劃線等）。 */
  summary: string[];
  canPrint: boolean;
  /** 列印用的另一份檔案（帶標註的 PDF 要把標註畫進頁面，列印服務不一定畫標註物件）；按列印時才準備。 */
  makePrintFile?: () => Promise<ExportFile>;
}

/** 組帶標註的 PDF 時 pdf-lib 讀不懂這份檔案。 */
class PdfError extends Error {}

/** 準備好之後：檔名與說明，交出面板。 */
function ReadySheet(props: { ready: Ready; onClose: () => void; children?: ReactNode }) {
  const { file, summary, canPrint, makePrintFile } = props.ready;
  return (
    <DeliverSheet
      title={t("export.ready.title")}
      file={file}
      primary="share"
      print={canPrint ? (makePrintFile ?? true) : false}
      closeLabel={t("export.done")}
      onClose={props.onClose}
    >
      <p className="export-file">{file.name}</p>
      {summary.map((s) => (
        <p key={s} className="muted small">
          {s}
        </p>
      ))}
      {props.children}
    </DeliverSheet>
  );
}

/**
 * 一篇文獻的匯出與列印。`materialId` 是目前在看的 PDF（沒給就是原文）；`overlayId` 是目前在看的譯文
 * （沒給就是最後一份）：匯出譯本、給翻譯軟體（XLIFF、TMX）都用它。
 */
export function ExportSheet(props: { paperId: string; materialId?: string; overlayId?: string; onClose: () => void }) {
  const { paperId } = props;
  const [materials, setMaterials] = useState<Material[] | null>(null);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState<Ready | null>(null);
  const [error, setError] = useState<{ message: string; detail?: string } | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .getPaper(paperId)
      .then((d) => {
        if (!alive) return;
        setMaterials(d.paper.materials);
        setTitle(d.paper.bib.title ?? d.paper.short_name ?? "");
      })
      .catch(() => alive && setMaterials([]));
    return () => {
      alive = false;
    };
  }, [paperId]);

  const source = materials?.find((m) => m.role === ROLE_SOURCE && m.form !== FORM_OVERLAY);
  const pdfId = props.materialId ?? source?.id;
  const overlays = materials?.filter((m) => m.form === FORM_OVERLAY) ?? [];
  const overlay = overlays.find((m) => m.id === props.overlayId) ?? overlays.at(-1);
  const digest = materials ? lastDigest(materials) : undefined;

  /** 準備一份檔案：做完交給 `then`；失敗顯示原因。 */
  const prepare = async (job: () => Promise<Ready | null>) => {
    setBusy(true);
    setError(null);
    try {
      const r = await job();
      if (r) setReady(r);
    } catch (e) {
      setError(
        e instanceof PdfError
          ? { message: t("export.pdfFailed"), detail: t("export.pdfFailed.detail") }
          : { message: errorMessage(kindOf(e)), detail: errorDetail(kindOf(e)) },
      );
    } finally {
      setBusy(false);
    }
  };

  const shareOriginal = () =>
    prepare(async () => {
      const file = await api.exportOriginal(paperId, pdfId ?? "");
      await api.deliverExport(file.path, file.name, "share");
      props.onClose();
      return null;
    });

  const printOriginal = () =>
    prepare(async () => {
      const file = await api.exportOriginal(paperId, pdfId ?? "");
      await api.deliverExport(file.path, file.name, "print");
      props.onClose();
      return null;
    });

  const annotated = () =>
    prepare(async () => {
      const built = await buildAnnotatedPdf(paperId, pdfId ?? "").catch((e: unknown) => {
        if (e instanceof AppError) throw e;
        throw new PdfError();
      });
      const file = await api.exportPutBytes(`${title.trim() || "paper"}-標註.pdf`, built.bytes);
      return {
        file,
        summary: [t("export.ready.annotatedSummary", { highlights: built.highlights, strokes: built.strokes })],
        canPrint: true,
        makePrintFile: async () => {
          const flat = await buildAnnotatedPdf(paperId, pdfId ?? "", { flatten: true });
          return api.exportPutBytes(`${title.trim() || "paper"}-列印.pdf`, flat.bytes);
        },
      };
    });

  const notes = () =>
    prepare(async () => {
      try {
        return { file: await api.exportNotes(paperId), summary: [], canPrint: false };
      } catch (e) {
        if (e instanceof AppError && e.kind === "NotFound") {
          setError({ message: t("export.noNotes") });
          return null;
        }
        throw e;
      }
    });

  /** 譯文：只有譯文（給別人讀）；`bilingual` 是原文與譯文逐段對照的那一版。 */
  const translation = (bilingual: boolean) =>
    prepare(async () => ({
      file: await api.exportTranslation(paperId, overlay?.id ?? "", bilingual),
      summary: [],
      canPrint: false,
    }));

  /** 給翻譯軟體：原文與譯文逐段寫成 XLIFF 2.1 或 TMX 1.4b。 */
  const interchange = (format: InterchangeFormat) =>
    prepare(async () => {
      const f = await api.overlayInterchange(paperId, overlay?.id ?? "", format);
      return {
        file: await api.exportPutBytes(f.name, new TextEncoder().encode(f.text)),
        summary: [t(`interchange.${format}.desc`)],
        canPrint: false,
      };
    });

  const digestFile = () =>
    prepare(async () => ({
      file: await api.exportDigest(paperId, digest?.id ?? ""),
      summary: [],
      canPrint: false,
    }));

  if (ready) return <ReadySheet ready={ready} onClose={props.onClose} />;

  const loading = materials === null;
  const noPdf = loading || !pdfId;
  const item = (label: string, hint: string | null, run: () => void, disabled: boolean) => (
    <button type="button" className="export-item" disabled={disabled || busy} onClick={run}>
      <span className="export-label">{label}</span>
      {hint ? <span className="muted small">{hint}</span> : null}
    </button>
  );

  return (
    <Sheet open title={t("export.title")} onDismiss={props.onClose}>
      {error ? <ErrorNotice message={error.message} detail={error.detail} /> : null}
      <div className="export-list">
        {item(t("export.shareOriginal"), null, () => void shareOriginal(), noPdf)}
        {item(t("export.annotated"), t("export.annotated.hint"), () => void annotated(), noPdf)}
        {item(t("export.notes"), null, () => void notes(), loading)}
        {item(
          t("export.translation"),
          overlay || loading ? t("export.translation.hint") : t("export.translation.none"),
          () => void translation(false),
          !overlay,
        )}
        {item(
          t("export.translation.bilingual"),
          overlay || loading ? null : t("export.translation.none"),
          () => void translation(true),
          !overlay,
        )}
        {item(t("export.digest"), digest || loading ? null : t("export.digest.none"), () => void digestFile(), !digest)}
        {(["xliff", "tmx"] as const).map((f) => (
          <Fragment key={f}>
            {item(
              t("interchange.item", { format: t(`interchange.${f}.name`) }),
              overlay || loading ? t(`interchange.${f}.desc`) : t("export.translation.none"),
              () => void interchange(f),
              !overlay,
            )}
          </Fragment>
        ))}
        {item(t("export.printOriginal"), null, () => void printOriginal(), noPdf)}
      </div>
      <div className="actions">
        <Button block onClick={props.onClose}>
          {t("export.close")}
        </Button>
      </div>
      <BusyOverlay active={busy} label={t("export.preparing")} />
    </Sheet>
  );
}

type BibChoice = { kind: "file"; format: BibFormat } | { kind: "style"; style: CitationStyle };

const FILE_FORMATS: BibFormat[] = ["bibtex", "ris", "csljson"];
const STYLES: CitationStyle[] = ["apa", "ieee", "chicago", "mla"];

/** 缺作者或年份的幾篇：點篇名關掉面板、打開那篇的文獻資訊去補。 */
function MissingList(props: { missing: Array<{ id: string; title: string }> }) {
  const nav = useContext(NavContext);
  if (props.missing.length === 0) return null;
  const open = async (id: string) => {
    if (!nav) return;
    await nav.back();
    nav.push({ name: "info", paperId: id });
  };
  return (
    <div className="bib-missing">
      <p className="muted small">{t("bib.missing", { n: props.missing.length })}</p>
      {props.missing.map((m) => (
        <button key={m.id} type="button" className="export-item" onClick={() => void open(m.id)}>
          <span className="export-label">{m.title || m.id}</span>
        </button>
      ))}
    </div>
  );
}

/** 幾篇文獻的書目一次匯出：書目檔（BibTeX、RIS、CSL-JSON）寫成一份檔案；引用格式（APA 等）複製或寫成引用清單。 */
export function BibExportSheet(props: {
  paperIds: string[];
  /** 檔名的前半（群組的名稱）；沒有就是「書目」。 */
  name: string | null;
  onClose: () => void;
}) {
  const [choice, setChoice] = useState<BibChoice>({ kind: "file", format: "bibtex" });
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState<Ready | null>(null);
  const [missing, setMissing] = useState<Array<{ id: string; title: string }>>([]);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<ErrorKind | null>(null);
  const n = props.paperIds.length;

  const run = async (job: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setCopied(null);
    try {
      await job();
    } catch (e) {
      setError(kindOf(e));
    } finally {
      setBusy(false);
    }
  };

  const exportFile = (format: BibFormat) =>
    run(async () => {
      const r = await api.exportBibliography(props.paperIds, format, props.name);
      const summary = [t("bib.ready.summary", { count: r.count })];
      if (r.skipped > 0) summary.push(t("bib.ready.skipped", { n: r.skipped }));
      setMissing([]);
      setReady({ file: r.file, summary, canPrint: false });
    });

  const citations = (style: CitationStyle, action: "copy" | "file") =>
    run(async () => {
      const r = await api.exportCitations(props.paperIds, style, action, props.name);
      const name = t(`bib.style.${style}`);
      setMissing(r.missing);
      if (r.file) {
        const summary = [t("bib.ready.citations", { count: r.count, style: name })];
        if (r.skipped > 0) summary.push(t("bib.ready.skipped", { n: r.skipped }));
        setReady({ file: r.file, summary, canPrint: false });
      } else {
        setCopied(t("bib.copied", { count: r.count, style: name }));
      }
    });

  if (ready) {
    return (
      <ReadySheet ready={ready} onClose={props.onClose}>
        <MissingList missing={missing} />
      </ReadySheet>
    );
  }

  const option = (key: string, selected: boolean, label: string, hint: string, pick: () => void) => (
    <Choice
      key={key}
      checked={selected}
      label={label}
      hint={hint}
      className="export-format"
      onPick={() => {
        setCopied(null);
        setMissing([]);
        pick();
      }}
    />
  );
  const isFile = choice.kind === "file";
  return (
    <Sheet open title={t("bib.title")} onDismiss={props.onClose}>
      <p>{n === 0 ? t("bib.empty") : t("bib.count", { n })}</p>
      <ChoiceList label={t("bib.format")}>
        <p className="muted small bib-group">{t("bib.files")}</p>
        {FILE_FORMATS.map((f) =>
          option(f, isFile && choice.format === f, t(`bib.format.${f}`), t(`bib.format.${f}.hint`), () =>
            setChoice({ kind: "file", format: f }),
          ),
        )}
        <p className="muted small bib-group">{t("bib.styles")}</p>
        {STYLES.map((st) =>
          option(st, !isFile && choice.style === st, t(`bib.style.${st}`), t(`bib.style.${st}.hint`), () =>
            setChoice({ kind: "style", style: st }),
          ),
        )}
      </ChoiceList>
      <p className="muted small">{isFile ? t("bib.note") : t("bib.style.note")}</p>
      {copied ? (
        <p className="notice" role="status">
          {copied}
        </p>
      ) : null}
      <MissingList missing={missing} />
      {error ? <KindNotice kind={error} /> : null}
      <div className="actions">
        {choice.kind === "file" ? (
          <Button primary block disabled={busy || n === 0} onClick={() => void exportFile(choice.format)}>
            {t("bib.go")}
          </Button>
        ) : (
          <>
            <Button primary block disabled={busy || n === 0} onClick={() => void citations(choice.style, "copy")}>
              {t("bib.copy")}
            </Button>
            <Button block disabled={busy || n === 0} onClick={() => void citations(choice.style, "file")}>
              {t("bib.list")}
            </Button>
          </>
        )}
        <Button block onClick={props.onClose}>
          {copied ? t("export.done") : t("common.cancel")}
        </Button>
      </div>
      <BusyOverlay active={busy} label={t("export.preparing")} />
    </Sheet>
  );
}
