import { useEffect, useRef, useState } from "react";
import type { HelpTopic, Nav } from "../../nav";
import { t, type StringKey } from "../../strings";
import { Button, Screen } from "../../ui/components";
import { resetHints } from "../../hints";

const TOPICS: Array<{ id: HelpTopic; heading: StringKey; lines: StringKey[] }> = [
  { id: "highlight", heading: "reader.edit.title", lines: ["help.highlight.1", "help.highlight.2", "help.highlight.3"] },
  { id: "brush", heading: "reader.tool.brush", lines: ["help.brush.1", "help.brush.2"] },
  { id: "toolbox", heading: "reader.toolbox", lines: ["help.toolbox.1", "help.toolbox.2", "help.toolbox.3"] },
  { id: "translate", heading: "help.translate.heading", lines: ["help.translate.1", "help.translate.2", "help.translate.3"] },
  {
    id: "obsidian",
    heading: "settings.notes.obsidian",
    lines: [
      "help.obsidian.1",
      "help.obsidian.2",
      "help.obsidian.3",
      "help.obsidian.4",
      "help.obsidian.5",
      "help.obsidian.6",
      "help.obsidian.7",
    ],
  },
];

/** 說明頁：要好幾步才說得清楚的操作集中在這裡；從設定或「了解更多」進來，`topic` 捲到那一節。 */
export function Help(props: { nav: Nav; topic?: HelpTopic }) {
  const refs = useRef<Partial<Record<HelpTopic, HTMLElement | null>>>({});
  const [reset, setReset] = useState(false);
  useEffect(() => {
    if (props.topic) refs.current[props.topic]?.scrollIntoView({ block: "start" });
  }, [props.topic]);
  return (
    <Screen title={t("help.title")} onBack={props.nav.back}>
      <div className="content help">
        {TOPICS.map((s) => (
          <section
            key={s.id}
            ref={(el) => {
              refs.current[s.id] = el;
            }}
          >
            <h2 className="section">{t(s.heading)}</h2>
            <ol>
              {s.lines.map((k) => (
                <li key={k}>{t(k)}</li>
              ))}
            </ol>
          </section>
        ))}
        <div className="actions">
          <Button
            block
            disabled={reset}
            onClick={() => {
              resetHints();
              setReset(true);
            }}
          >
            {reset ? t("help.hints.done") : t("help.hints.reset")}
          </Button>
        </div>
      </div>
    </Screen>
  );
}
