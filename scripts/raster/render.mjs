// Rasterise one SVG to PNG with resvg. Used by scripts/bake_diagrams.py, because the
// machine has neither fonts nor cairo/rsvg/inkscape, and the blog's inline SVG diagrams
// need a PNG twin for readers and clients that cannot render SVG.
//
// Usage: node render.mjs <input.svg> <output.png> [zoom]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { Resvg } from "@resvg/resvg-js";

const [input, output, zoom] = process.argv.slice(2);
if (!input || !output) {
  console.error("usage: node render.mjs <input.svg> <output.png> [zoom]");
  process.exit(2);
}

// No system fonts exist on the build host, so ship the faces with the tool. The diagram
// captions are Chinese, so the faces are Noto Sans SC (400 + 600).
const fonts = dirname(fileURLToPath(import.meta.url));
const svg = readFileSync(input, "utf8");
const resvg = new Resvg(svg, {
  fitTo: { mode: "zoom", value: Number(zoom ?? 2) },
  font: {
    fontFiles: [
      join(fonts, "fonts", "NotoSansSC_400Regular.ttf"),
      join(fonts, "fonts", "NotoSansSC_600SemiBold.ttf"),
    ],
    loadSystemFonts: false,
    defaultFontFamily: "Noto Sans SC",
    sansSerifFamily: "Noto Sans SC",
    // The request-id labels ask for `ui-monospace, SFMono-Regular, Menlo, monospace`.
    // None of those exist here, so point the generic at the family we do ship —
    // otherwise those few ASCII labels come out blank.
    monospaceFamily: "Noto Sans SC",
  },
});

writeFileSync(output, resvg.render().asPng());
