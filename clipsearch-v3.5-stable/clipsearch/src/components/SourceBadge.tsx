// Source badge — small colored pill identifying the result source.
//
// Five sources, each with a distinct color:
//   clipboard — blue
//   note      — purple
//   file      — sky
//   app       — green
//   web       — orange

import { t } from "@/i18n";

interface Props {
  source: "clipboard" | "note" | "file" | "app" | "web";
}

export function SourceBadge({ source }: Props) {
  const label = (() => {
    switch (source) {
      case "clipboard":
        return t("source.clipboard");
      case "note":
        return t("source.note");
      case "file":
        return "FILE";
      case "app":
        return "APP";
      case "web":
        return "WEB";
    }
  })();

  const cls = (() => {
    switch (source) {
      case "clipboard":
        return "bg-blue-600 text-white";
      case "note":
        return "bg-purple-600 text-white";
      case "file":
        return "bg-sky-600 text-white";
      case "app":
        return "bg-green-600 text-white";
      case "web":
        return "bg-orange-600 text-white";
    }
  })();

  return (
    <span
      className={`tnum text-[10px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded ${cls}`}
    >
      {label}
    </span>
  );
}
