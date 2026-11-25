import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const fontPath = fileURLToPath(new URL("./Abel-Regular.ttf", import.meta.url));
const font = await readFile(fontPath);

const u16 = (offset) => font.readUInt16BE(offset);
const i16 = (offset) => font.readInt16BE(offset);
const u32 = (offset) => font.readUInt32BE(offset);

const tables = new Map();
const tableCount = u16(4);
for (let index = 0; index < tableCount; index += 1) {
  const record = 12 + index * 16;
  const tag = font.toString("ascii", record, record + 4);
  tables.set(tag, {
    offset: u32(record + 8),
    length: u32(record + 12),
  });
}

function table(tag) {
  const value = tables.get(tag);
  if (!value) {
    throw new Error(`missing ${tag} table`);
  }
  return value;
}

function cmapFormat4Glyph(subtable, codepoint) {
  const segmentCount = u16(subtable + 6) / 2;
  const endCodes = subtable + 14;
  const startCodes = endCodes + segmentCount * 2 + 2;
  const deltas = startCodes + segmentCount * 2;
  const rangeOffsets = deltas + segmentCount * 2;

  for (let index = 0; index < segmentCount; index += 1) {
    const start = u16(startCodes + index * 2);
    const end = u16(endCodes + index * 2);
    if (codepoint < start || codepoint > end) {
      continue;
    }

    const delta = i16(deltas + index * 2);
    const rangeOffsetAddress = rangeOffsets + index * 2;
    const rangeOffset = u16(rangeOffsetAddress);
    if (rangeOffset === 0) {
      return (codepoint + delta) & 0xffff;
    }

    const glyphAddress =
      rangeOffsetAddress + rangeOffset + (codepoint - start) * 2;
    const glyph = u16(glyphAddress);
    return glyph === 0 ? 0 : (glyph + delta) & 0xffff;
  }
  return 0;
}

function glyphForCodepoint(codepoint) {
  const cmap = table("cmap").offset;
  const encodingCount = u16(cmap + 2);
  for (let index = 0; index < encodingCount; index += 1) {
    const record = cmap + 4 + index * 8;
    const platform = u16(record);
    const encoding = u16(record + 2);
    if (platform !== 0 && !(platform === 3 && encoding === 1)) {
      continue;
    }

    const subtable = cmap + u32(record + 4);
    if (u16(subtable) === 4) {
      const glyph = cmapFormat4Glyph(subtable, codepoint);
      if (glyph !== 0) {
        return glyph;
      }
    }
  }
  throw new Error(`no format-4 cmap entry for U+${codepoint.toString(16)}`);
}

const head = table("head").offset;
const unitsPerEm = u16(head + 18);
const locaFormat = i16(head + 50);
const glyphId = glyphForCodepoint("M".codePointAt(0));
const loca = table("loca").offset;
const glyphOffset =
  locaFormat === 0 ? u16(loca + glyphId * 2) * 2 : u32(loca + glyphId * 4);
const glyph = table("glyf").offset + glyphOffset;
const bbox = {
  x_min: i16(glyph + 2),
  y_min: i16(glyph + 4),
  x_max: i16(glyph + 6),
  y_max: i16(glyph + 8),
};
bbox.width = bbox.x_max - bbox.x_min;
bbox.height = bbox.y_max - bbox.y_min;
bbox.area = bbox.width * bbox.height;

const hhea = table("hhea").offset;
const metricCount = u16(hhea + 34);
const hmtx = table("hmtx").offset;
const metricIndex = Math.min(glyphId, metricCount - 1);
const advanceWidth = u16(hmtx + metricIndex * 4);

const sizes = [12, 16, 24, 48].map((px) => {
  const scale = px / unitsPerEm;
  const scaledInkArea = bbox.area * scale * scale;
  const threshold = Math.ceil(
    Math.min(scaledInkArea, Math.max(10, scaledInkArea * 0.1)),
  );
  const rounded = {
    x_min: Math.floor(bbox.x_min * scale),
    y_min: Math.floor(bbox.y_min * scale),
    x_max: Math.ceil(bbox.x_max * scale),
    y_max: Math.ceil(bbox.y_max * scale),
  };
  rounded.area =
    (rounded.x_max - rounded.x_min) * (rounded.y_max - rounded.y_min);
  return {
    px,
    scaled_ink_area: scaledInkArea,
    threshold,
    outward_rounded_bbox: rounded,
  };
});

const calibration = {
  font: "Abel-Regular.ttf",
  glyph: "M",
  codepoint: "M".codePointAt(0),
  glyph_id: glyphId,
  advance_width: advanceWidth,
  units_per_em: unitsPerEm,
  number_of_contours: i16(glyph),
  font_unit_bbox: bbox,
  sizes,
};

process.stdout.write(`${JSON.stringify(calibration, null, 2)}\n`);
