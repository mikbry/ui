//! Narrow, from-scratch SFNT/TrueType provider for the Sprint 7 Slug slice.
//!
//! This module decodes exactly the slice of the SFNT container the checked-in
//! `Abel-Regular.ttf` fixture needs — and nothing more — so the workspace keeps
//! its "own the stack" commitment (no runtime `ttf-parser`/`freetype`/shaping
//! dependency; the crate-level forbid-list in [`crate`] applies here too).
//! `ttf-parser` appears only as a **dev-dependency test oracle**, never in this
//! parser.
//!
//! ## Deliberately narrow runtime scope
//!
//! - face index 0 of a single-font `sfnt` (or the index-0 member of a `ttcf`
//!   collection),
//! - the SFNT table directory, validated for in-bounds offsets/lengths,
//! - `head` / `maxp` / `hhea` / `hmtx` for metrics, `cmap` **format 4** (the
//!   format the fixture's Unicode subtable uses), `loca` + `glyf` for simple
//!   TrueType quadratic outlines, and minimal `name` data for the family,
//! - Unicode scalar → glyph id for the ASCII/Latin showcase set,
//! - advances and simple quadratic outlines, y-up in font units.
//!
//! Everything outside that slice is a **typed** [`SfntError`]: CFF/CFF2 outlines
//! ([`SfntError::UnsupportedOutlineFormat`]), color/bitmap glyph tables
//! ([`SfntError::UnsupportedColorGlyphs`]), composite glyphs
//! ([`SfntError::CompositeGlyph`]), malformed/truncated tables
//! ([`SfntError::Malformed`]), and unmapped characters (`None` from
//! [`SfntFace::glyph_index`], surfaced at layout as bitmap fallback).
//!
//! ## Identity boundary
//!
//! [`SfntFace`] is a pure decoder — it never mints a [`FontId`]. The provider
//! adapter ([`SfntProvider`]) plugs into #62's registry, which owns the shared
//! [`FontIdAllocator`](crate::FontIdAllocator) and is the single authority for
//! the public [`FontId`] / [`TextRenderClass`] of a face. Glyphs the face lacks
//! are split out at **layout time** into registry-validated bitmap-fallback
//! runs — never dropped, never silently substituted after layout.

use std::sync::Arc;

use crate::canonical::Affine2Fixed;
use crate::outline::{GlyphOutline, OutlineBounds, OutlineCommand};

/// Typed failure modes of the narrow SFNT decoder.
///
/// The variants map 1:1 to the "rejects unsupported/malformed inputs with typed
/// errors" acceptance criterion: each is a distinct, matchable reason the
/// fixture-narrow parser declined, never a generic catch-all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SfntError {
    /// The byte blob is shorter than a structure it must contain, or a table
    /// offset/length points outside the blob. Carries a short static label for
    /// the structure that failed its bounds check.
    #[error("malformed or truncated SFNT: {0}")]
    Malformed(&'static str),
    /// The `sfnt` version tag is not one this decoder accepts (`0x00010000`,
    /// `true`, or `ttcf`).
    #[error("unrecognized sfnt version: {0:#010x}")]
    UnknownSfntVersion(u32),
    /// A required table is absent from the directory.
    #[error("missing required SFNT table: {0}")]
    MissingTable(&'static str),
    /// The requested face index is past the end of a `ttcf` collection (or
    /// nonzero for a single-font file).
    #[error("face index {0} out of range")]
    FaceIndexOutOfRange(u32),
    /// The font carries PostScript/CFF outlines (`CFF ` / `CFF2`) rather than
    /// TrueType `glyf` outlines, which this narrow decoder does not parse.
    #[error("CFF/CFF2 outlines are not supported by the narrow SFNT decoder")]
    UnsupportedOutlineFormat,
    /// The font's primary glyph source is a color or embedded-bitmap table
    /// (`CBDT`/`CBLC`/`sbix`/`COLR`), unsupported here.
    #[error("color/bitmap glyph tables are not supported by the narrow SFNT decoder")]
    UnsupportedColorGlyphs,
    /// No Unicode `cmap` subtable in a format this decoder understands
    /// (format 4) was found.
    #[error("no supported Unicode cmap subtable (format 4) found")]
    UnsupportedCmap,
    /// A glyph id was requested that is past `maxp.numGlyphs`.
    #[error("glyph id {0} out of range")]
    GlyphOutOfRange(u16),
    /// The glyph is a composite (component) glyph. The fixture's showcase set
    /// needs only simple glyphs, so composites are an explicit typed rejection.
    #[error("composite glyphs are not supported by the narrow SFNT decoder")]
    CompositeGlyph,
}

/// A four-byte big-endian read of an SFNT table tag.
fn read_tag(data: &[u8], off: usize) -> Result<[u8; 4], SfntError> {
    data.get(off..off + 4)
        .map(|s| [s[0], s[1], s[2], s[3]])
        .ok_or(SfntError::Malformed("table tag"))
}

fn read_u16(data: &[u8], off: usize) -> Result<u16, SfntError> {
    data.get(off..off + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or(SfntError::Malformed("u16"))
}

fn read_i16(data: &[u8], off: usize) -> Result<i16, SfntError> {
    read_u16(data, off).map(|v| v as i16)
}

fn read_u32(data: &[u8], off: usize) -> Result<u32, SfntError> {
    data.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(SfntError::Malformed("u32"))
}

/// Offset + length of one table within the blob, validated in bounds.
#[derive(Debug, Clone, Copy)]
struct TableRecord {
    offset: usize,
    length: usize,
}

impl TableRecord {
    /// The table's bytes, re-validated against the blob length.
    fn slice<'a>(&self, data: &'a [u8], name: &'static str) -> Result<&'a [u8], SfntError> {
        data.get(self.offset..self.offset + self.length)
            .ok_or(SfntError::Malformed(name))
    }
}

/// A decoded SFNT face — the narrow TrueType slice the fixture exercises.
///
/// Owns its backing bytes via `Arc<[u8]>` so the parsed face is
/// `Send + Sync + 'static` and can live behind the registry's
/// `Arc<dyn TextSystem>` / provider boxes. Table offsets are validated at parse
/// time; per-glyph `glyf` data is decoded on demand.
#[derive(Debug, Clone)]
pub struct SfntFace {
    data: Arc<[u8]>,
    units_per_em: u16,
    num_glyphs: u16,
    index_to_loc_long: bool,
    number_of_h_metrics: u16,
    ascender: i16,
    descender: i16,
    line_gap: i16,
    cmap_format4: TableRecord,
    loca: TableRecord,
    glyf: TableRecord,
    hmtx: TableRecord,
    family_name: Option<String>,
}

/// Resolved horizontal/vertical line metrics for the face, in font units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfntMetrics {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
}

impl SfntFace {
    /// Decode face `index` from a single-font `sfnt` or a `ttcf` collection.
    ///
    /// Validates the table directory and every required table's bounds up
    /// front; returns a typed [`SfntError`] for anything outside the narrow
    /// supported slice. `index` must be `0` for a single-font file.
    pub fn parse(bytes: Arc<[u8]>, index: u32) -> Result<Self, SfntError> {
        let table_dir = Self::table_directory_offset(&bytes, index)?;
        let tables = Self::read_table_directory(&bytes, table_dir)?;

        let find = |tag: &[u8; 4]| tables.iter().find(|(t, _)| t == tag).map(|(_, r)| *r);

        // Reject non-TrueType glyph sources with distinct typed errors rather
        // than a generic "missing glyf": the *reason* is load-bearing.
        if find(b"glyf").is_none() || find(b"loca").is_none() {
            if find(b"CFF ").is_some() || find(b"CFF2").is_some() {
                return Err(SfntError::UnsupportedOutlineFormat);
            }
            if find(b"CBDT").is_some()
                || find(b"CBLC").is_some()
                || find(b"sbix").is_some()
                || find(b"COLR").is_some()
            {
                return Err(SfntError::UnsupportedColorGlyphs);
            }
            return Err(SfntError::MissingTable("glyf/loca"));
        }

        let head = find(b"head").ok_or(SfntError::MissingTable("head"))?;
        let maxp = find(b"maxp").ok_or(SfntError::MissingTable("maxp"))?;
        let hhea = find(b"hhea").ok_or(SfntError::MissingTable("hhea"))?;
        let hmtx = find(b"hmtx").ok_or(SfntError::MissingTable("hmtx"))?;
        let cmap = find(b"cmap").ok_or(SfntError::MissingTable("cmap"))?;
        let loca = find(b"loca").ok_or(SfntError::MissingTable("loca"))?;
        let glyf = find(b"glyf").ok_or(SfntError::MissingTable("glyf"))?;

        // `head`: units-per-em (offset 18) and the loca index format (offset
        // 50: 0 = short u16×2, 1 = long u32).
        let head_bytes = head.slice(&bytes, "head")?;
        let units_per_em = read_u16(head_bytes, 18)?;
        if units_per_em == 0 {
            return Err(SfntError::Malformed("head.unitsPerEm == 0"));
        }
        let index_to_loc_long = match read_i16(head_bytes, 50)? {
            0 => false,
            1 => true,
            _ => return Err(SfntError::Malformed("head.indexToLocFormat")),
        };

        let num_glyphs = read_u16(maxp.slice(&bytes, "maxp")?, 4)?;

        // `hhea`: ascender (4), descender (6), lineGap (8), numberOfHMetrics
        // (34) — the count of full advance entries in `hmtx`.
        let hhea_bytes = hhea.slice(&bytes, "hhea")?;
        let ascender = read_i16(hhea_bytes, 4)?;
        let descender = read_i16(hhea_bytes, 6)?;
        let line_gap = read_i16(hhea_bytes, 8)?;
        let number_of_h_metrics = read_u16(hhea_bytes, 34)?;
        if number_of_h_metrics == 0 {
            return Err(SfntError::Malformed("hhea.numberOfHMetrics == 0"));
        }

        let cmap_format4 = Self::find_unicode_cmap_format4(&bytes, cmap)?;
        let family_name = find(b"name").and_then(|name| Self::read_family_name(&bytes, name).ok());

        Ok(Self {
            data: bytes,
            units_per_em,
            num_glyphs,
            index_to_loc_long,
            number_of_h_metrics,
            ascender,
            descender,
            line_gap,
            cmap_format4,
            loca,
            glyf,
            hmtx,
            family_name,
        })
    }

    /// Offset of the table directory for face `index`, handling the `ttcf`
    /// collection header. A single-font file accepts only index 0.
    fn table_directory_offset(data: &[u8], index: u32) -> Result<usize, SfntError> {
        let version = read_u32(data, 0)?;
        match version {
            // `ttcf`: collection header — numFonts (8), offsets[] (12..).
            0x7474_6366 => {
                let num_fonts = read_u32(data, 8)?;
                if index >= num_fonts {
                    return Err(SfntError::FaceIndexOutOfRange(index));
                }
                let off = read_u32(data, 12 + index as usize * 4)? as usize;
                // The per-face offset table itself carries an sfnt version.
                match read_u32(data, off)? {
                    0x0001_0000 | 0x7472_7565 => Ok(off),
                    0x4F54_544F => Err(SfntError::UnsupportedOutlineFormat),
                    other => Err(SfntError::UnknownSfntVersion(other)),
                }
            }
            // Single-font TrueType (`0x00010000`) or the legacy `true` tag.
            0x0001_0000 | 0x7472_7565 => {
                if index != 0 {
                    return Err(SfntError::FaceIndexOutOfRange(index));
                }
                Ok(0)
            }
            // `OTTO`: OpenType with CFF outlines — narrow decoder declines.
            0x4F54_544F => Err(SfntError::UnsupportedOutlineFormat),
            other => Err(SfntError::UnknownSfntVersion(other)),
        }
    }

    /// Parse the `(tag, record)` list of the table directory at `dir_off`,
    /// validating every record's bounds against the blob.
    fn read_table_directory(
        data: &[u8],
        dir_off: usize,
    ) -> Result<Vec<([u8; 4], TableRecord)>, SfntError> {
        let num_tables = read_u16(data, dir_off + 4)? as usize;
        let mut tables = Vec::with_capacity(num_tables);
        for i in 0..num_tables {
            let rec = dir_off + 12 + i * 16;
            let tag = read_tag(data, rec)?;
            let offset = read_u32(data, rec + 8)? as usize;
            let length = read_u32(data, rec + 12)? as usize;
            // Validate in-bounds now so later table reads can trust the record.
            if offset
                .checked_add(length)
                .is_none_or(|end| end > data.len())
            {
                return Err(SfntError::Malformed("table record out of bounds"));
            }
            tables.push((tag, TableRecord { offset, length }));
        }
        Ok(tables)
    }

    /// Locate a Unicode `cmap` format-4 subtable, preferring Windows BMP
    /// (platform 3 / encoding 1) then Unicode (platform 0). Returns the
    /// subtable as a [`TableRecord`].
    fn find_unicode_cmap_format4(data: &[u8], cmap: TableRecord) -> Result<TableRecord, SfntError> {
        let base = cmap.offset;
        let num_subtables = read_u16(data, base + 2)? as usize;
        let mut best: Option<usize> = None;
        let mut best_rank = u8::MAX;
        for i in 0..num_subtables {
            let rec = base + 4 + i * 8;
            let platform = read_u16(data, rec)?;
            let encoding = read_u16(data, rec + 2)?;
            let sub_off = base + read_u32(data, rec + 4)? as usize;
            // Only Unicode-capable subtables, and only format 4.
            let unicode = matches!((platform, encoding), (3, 1) | (3, 10) | (0, _));
            if !unicode {
                continue;
            }
            if read_u16(data, sub_off).unwrap_or(0) != 4 {
                continue;
            }
            // Prefer Windows BMP (rank 0), then any Unicode platform (rank 1).
            let rank = if platform == 3 { 0 } else { 1 };
            if rank < best_rank {
                best_rank = rank;
                best = Some(sub_off);
            }
        }
        let sub_off = best.ok_or(SfntError::UnsupportedCmap)?;
        // The subtable's own length field bounds the format-4 arrays.
        let length = read_u16(data, sub_off + 2)? as usize;
        if sub_off + length > data.len() {
            return Err(SfntError::Malformed("cmap subtable out of bounds"));
        }
        Ok(TableRecord {
            offset: sub_off,
            length,
        })
    }

    /// Read the family name (name id 1) as UTF-8, preferring a Windows
    /// UTF-16BE record then a Macintosh Roman record. Best-effort: a font with
    /// an unreadable name table simply reports `None`.
    fn read_family_name(data: &[u8], name: TableRecord) -> Result<String, SfntError> {
        let base = name.offset;
        let count = read_u16(data, base + 2)? as usize;
        let string_offset = base + read_u16(data, base + 4)? as usize;
        let mut mac_fallback: Option<String> = None;
        for i in 0..count {
            let rec = base + 6 + i * 12;
            let platform = read_u16(data, rec)?;
            let _encoding = read_u16(data, rec + 2)?;
            let name_id = read_u16(data, rec + 6)?;
            if name_id != 1 {
                continue;
            }
            let length = read_u16(data, rec + 8)? as usize;
            let str_off = string_offset + read_u16(data, rec + 10)? as usize;
            let raw = data
                .get(str_off..str_off + length)
                .ok_or(SfntError::Malformed("name string"))?;
            match platform {
                // Windows: UTF-16BE.
                3 => {
                    let units: Vec<u16> = raw
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    if let Ok(s) = String::from_utf16(&units) {
                        return Ok(s);
                    }
                }
                // Macintosh Roman (ASCII subset for Latin names) — kept as a
                // fallback so a Windows record always wins when present.
                1 if mac_fallback.is_none() && raw.is_ascii() => {
                    mac_fallback = Some(String::from_utf8_lossy(raw).into_owned());
                }
                _ => {}
            }
        }
        mac_fallback.ok_or(SfntError::MissingTable("name id 1"))
    }

    /// Font design units per em.
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Number of glyphs in the face (`maxp.numGlyphs`).
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Resolved line metrics, in font units.
    pub fn metrics(&self) -> SfntMetrics {
        SfntMetrics {
            units_per_em: self.units_per_em,
            ascender: self.ascender,
            descender: self.descender,
            line_gap: self.line_gap,
        }
    }

    /// The family name from the `name` table, if it was readable.
    pub fn family_name(&self) -> Option<&str> {
        self.family_name.as_deref()
    }

    /// Map a Unicode scalar to a glyph id via the format-4 cmap, or `None` if
    /// the character is unmapped (the layout-time bitmap-fallback boundary).
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        let cp = ch as u32;
        // Format 4 only addresses the BMP; anything above is unmapped here.
        if cp > 0xFFFF {
            return None;
        }
        self.cmap_format4_lookup(cp as u16).filter(|&g| g != 0)
    }

    /// Format-4 segment search. Layout mirrors the Apple/Microsoft spec:
    /// `segCountX2` (6), then `endCode[]`, a reserved pad, `startCode[]`,
    /// `idDelta[]`, and `idRangeOffset[]`.
    fn cmap_format4_lookup(&self, cp: u16) -> Option<u16> {
        let data = &self.data;
        let base = self.cmap_format4.offset;
        let seg_count = read_u16(data, base + 6).ok()? as usize / 2;
        let end_codes = base + 14;
        let start_codes = end_codes + seg_count * 2 + 2;
        let id_deltas = start_codes + seg_count * 2;
        let id_range_offsets = id_deltas + seg_count * 2;

        for seg in 0..seg_count {
            let end = read_u16(data, end_codes + seg * 2).ok()?;
            if cp > end {
                continue;
            }
            let start = read_u16(data, start_codes + seg * 2).ok()?;
            if cp < start {
                return None; // Segments are sorted; a gap means unmapped.
            }
            let delta = read_u16(data, id_deltas + seg * 2).ok()?;
            let range_offset_addr = id_range_offsets + seg * 2;
            let range_offset = read_u16(data, range_offset_addr).ok()?;
            if range_offset == 0 {
                return Some(((cp as u32 + delta as u32) & 0xFFFF) as u16);
            }
            // Indirect: index into the glyph-id array that trails the
            // idRangeOffset table.
            let glyph_addr = range_offset_addr + range_offset as usize + (cp - start) as usize * 2;
            let glyph = read_u16(data, glyph_addr).ok()?;
            if glyph == 0 {
                return Some(0);
            }
            return Some(((glyph as u32 + delta as u32) & 0xFFFF) as u16);
        }
        None
    }

    /// Horizontal advance width of `glyph_id`, in font units. Per the `hmtx`
    /// spec, ids at or beyond `numberOfHMetrics` reuse the last full advance
    /// (monospaced tail).
    pub fn advance_width(&self, glyph_id: u16) -> u16 {
        let metric = glyph_id.min(self.number_of_h_metrics - 1) as usize;
        let off = self.hmtx.offset + metric * 4;
        read_u16(&self.data, off).unwrap_or(0)
    }

    /// `(offset, length)` of `glyph_id` within the `glyf` table, from `loca`.
    fn glyf_range(&self, glyph_id: u16) -> Result<(usize, usize), SfntError> {
        if glyph_id >= self.num_glyphs {
            return Err(SfntError::GlyphOutOfRange(glyph_id));
        }
        let gid = glyph_id as usize;
        let (start, end) = if self.index_to_loc_long {
            let o = self.loca.offset;
            (
                read_u32(&self.data, o + gid * 4)? as usize,
                read_u32(&self.data, o + (gid + 1) * 4)? as usize,
            )
        } else {
            // Short loca stores half-offsets.
            let o = self.loca.offset;
            (
                read_u16(&self.data, o + gid * 2)? as usize * 2,
                read_u16(&self.data, o + (gid + 1) * 2)? as usize * 2,
            )
        };
        if end < start {
            return Err(SfntError::Malformed("loca offsets out of order"));
        }
        Ok((self.glyf.offset + start, end - start))
    }

    /// Decode the simple TrueType outline of `glyph_id` into a fully-resolved
    /// [`GlyphOutline`], font units **y-up**, with `ink_bounds` matching the
    /// emitted points.
    ///
    /// An empty glyph (e.g. the space glyph: `loca[gid] == loca[gid+1]`)
    /// resolves to an outline with no commands and zero bounds. Composite
    /// glyphs are an explicit [`SfntError::CompositeGlyph`].
    pub fn glyph_outline(&self, glyph_id: u16) -> Result<GlyphOutline, SfntError> {
        let (offset, length) = self.glyf_range(glyph_id)?;
        if length == 0 {
            // No contour data — a blank glyph (space). Valid, draws nothing.
            return Ok(GlyphOutline {
                units_per_em: self.units_per_em,
                ink_bounds: OutlineBounds::default(),
                commands: Vec::new(),
            });
        }
        let number_of_contours = read_i16(&self.data, offset)?;
        if number_of_contours < 0 {
            return Err(SfntError::CompositeGlyph);
        }
        self.decode_simple_glyph(offset, number_of_contours as usize)
    }

    /// Decode a simple-glyph body (header already read) into outline commands.
    fn decode_simple_glyph(
        &self,
        offset: usize,
        contours: usize,
    ) -> Result<GlyphOutline, SfntError> {
        let data = &self.data;
        // Header is 10 bytes (numberOfContours + xMin/yMin/xMax/yMax).
        let mut cursor = offset + 10;

        // endPtsOfContours[contours]; the last entry + 1 is the point count.
        let mut end_pts = Vec::with_capacity(contours);
        for _ in 0..contours {
            end_pts.push(read_u16(data, cursor)?);
            cursor += 2;
        }
        let num_points = end_pts.last().map(|&e| e as usize + 1).unwrap_or(0);

        // Skip TrueType hinting instructions.
        let instruction_len = read_u16(data, cursor)? as usize;
        cursor += 2 + instruction_len;

        // Flags, run-length expanded via the REPEAT bit.
        const ON_CURVE: u8 = 0x01;
        const X_SHORT: u8 = 0x02;
        const Y_SHORT: u8 = 0x04;
        const REPEAT: u8 = 0x08;
        const X_SAME_OR_POS: u8 = 0x10;
        const Y_SAME_OR_POS: u8 = 0x20;

        let mut flags = Vec::with_capacity(num_points);
        while flags.len() < num_points {
            let flag = *data.get(cursor).ok_or(SfntError::Malformed("glyf flags"))?;
            cursor += 1;
            flags.push(flag);
            if flag & REPEAT != 0 {
                let repeat = *data
                    .get(cursor)
                    .ok_or(SfntError::Malformed("glyf repeat"))?;
                cursor += 1;
                for _ in 0..repeat {
                    if flags.len() >= num_points {
                        break;
                    }
                    flags.push(flag);
                }
            }
        }

        // X then Y deltas, each either short (u8 + sign bit) or i16, or "same".
        let mut xs = Vec::with_capacity(num_points);
        let mut x = 0i32;
        for &flag in &flags {
            if flag & X_SHORT != 0 {
                let d = *data.get(cursor).ok_or(SfntError::Malformed("glyf x"))? as i32;
                cursor += 1;
                x += if flag & X_SAME_OR_POS != 0 { d } else { -d };
            } else if flag & X_SAME_OR_POS == 0 {
                x += read_i16(data, cursor)? as i32;
                cursor += 2;
            }
            xs.push(x);
        }
        let mut ys = Vec::with_capacity(num_points);
        let mut y = 0i32;
        for &flag in &flags {
            if flag & Y_SHORT != 0 {
                let d = *data.get(cursor).ok_or(SfntError::Malformed("glyf y"))? as i32;
                cursor += 1;
                y += if flag & Y_SAME_OR_POS != 0 { d } else { -d };
            } else if flag & Y_SAME_OR_POS == 0 {
                y += read_i16(data, cursor)? as i32;
                cursor += 2;
            }
            ys.push(y);
        }

        // Walk each contour into quadratic commands, inserting the implied
        // on-curve midpoints between consecutive off-curve points.
        let mut commands = Vec::new();
        let mut start_pt = 0usize;
        for &end in &end_pts {
            let end = end as usize;
            if end < start_pt || end >= num_points {
                return Err(SfntError::Malformed("glyf contour endpoint"));
            }
            emit_contour(
                &mut commands,
                &xs[start_pt..=end],
                &ys[start_pt..=end],
                &flags[start_pt..=end],
                ON_CURVE,
            );
            start_pt = end + 1;
        }

        let ink_bounds = bounds_of(&commands);
        Ok(GlyphOutline {
            units_per_em: self.units_per_em,
            ink_bounds,
            commands,
        })
    }
}

/// A point as `(x, y, on_curve)` in font units.
#[derive(Clone, Copy)]
struct Pt {
    x: f32,
    y: f32,
    on: bool,
}

fn midpoint(a: Pt, b: Pt) -> Pt {
    Pt {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
        on: true,
    }
}

/// Emit one closed contour's [`OutlineCommand`]s, following the TrueType rule
/// that two consecutive off-curve points imply an on-curve midpoint between
/// them. Produces `MoveTo … (QuadTo|LineTo)* Close`.
fn emit_contour(out: &mut Vec<OutlineCommand>, xs: &[i32], ys: &[i32], flags: &[u8], on_bit: u8) {
    let n = xs.len();
    if n == 0 {
        return;
    }
    let pts: Vec<Pt> = (0..n)
        .map(|i| Pt {
            x: xs[i] as f32,
            y: ys[i] as f32,
            on: flags[i] & on_bit != 0,
        })
        .collect();

    // Establish an on-curve starting point. If the first point is off-curve,
    // use the last on-curve point, or the implied midpoint of the first/last
    // off-curve pair when the contour is all-off-curve.
    let (start, start_index) = if pts[0].on {
        (pts[0], 1)
    } else if pts[n - 1].on {
        (pts[n - 1], 0)
    } else {
        (midpoint(pts[n - 1], pts[0]), 0)
    };

    out.push(OutlineCommand::MoveTo {
        x: start.x,
        y: start.y,
    });

    let mut pending: Option<Pt> = None;
    // Iterate the contour points once, wrapping back to `start` to close.
    for step in 0..n {
        let idx = (start_index + step) % n;
        let p = pts[idx];
        if p.on {
            match pending.take() {
                Some(ctrl) => out.push(OutlineCommand::QuadTo {
                    cx: ctrl.x,
                    cy: ctrl.y,
                    x: p.x,
                    y: p.y,
                }),
                None => out.push(OutlineCommand::LineTo { x: p.x, y: p.y }),
            }
        } else {
            match pending.take() {
                // Two off-curve in a row: emit through their implied midpoint.
                Some(ctrl) => {
                    let mid = midpoint(ctrl, p);
                    out.push(OutlineCommand::QuadTo {
                        cx: ctrl.x,
                        cy: ctrl.y,
                        x: mid.x,
                        y: mid.y,
                    });
                    pending = Some(p);
                }
                None => pending = Some(p),
            }
        }
    }
    // Close back onto the start point, flushing any trailing control point.
    // With no pending control, MoveTo already sits on `start` and Close
    // re-joins it, so no explicit segment is needed.
    if let Some(ctrl) = pending.take() {
        out.push(OutlineCommand::QuadTo {
            cx: ctrl.x,
            cy: ctrl.y,
            x: start.x,
            y: start.y,
        });
    }
    out.push(OutlineCommand::Close);
}

/// Axis-aligned bounds over every on- and off-curve point an outline emits.
/// Control points are included so the bounds enclose the curve's control hull,
/// matching the convention an outline rasterizer reports.
fn bounds_of(commands: &[OutlineCommand]) -> OutlineBounds {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut acc = |x: f32, y: f32| {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };
    for cmd in commands {
        match *cmd {
            OutlineCommand::MoveTo { x, y } | OutlineCommand::LineTo { x, y } => acc(x, y),
            OutlineCommand::QuadTo { cx, cy, x, y } => {
                acc(cx, cy);
                acc(x, y);
            }
            OutlineCommand::Close => {}
        }
    }
    if min_x > max_x {
        OutlineBounds::default()
    } else {
        OutlineBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
}

/// Apply a canonical outline-local affine to a resolved outline in place,
/// recomputing `ink_bounds` from the transformed points so they stay matched.
/// A no-op when `transform` is the identity (the static-Abel slice).
pub(crate) fn apply_transform(outline: &mut GlyphOutline, transform: Affine2Fixed) {
    if transform.is_identity() {
        return;
    }
    let a = transform.a.to_f32();
    let b = transform.b.to_f32();
    let c = transform.c.to_f32();
    let d = transform.d.to_f32();
    let tx = transform.tx.to_f32();
    let ty = transform.ty.to_f32();
    let map = |x: f32, y: f32| (a * x + c * y + tx, b * x + d * y + ty);
    for cmd in &mut outline.commands {
        match cmd {
            OutlineCommand::MoveTo { x, y } | OutlineCommand::LineTo { x, y } => {
                let (nx, ny) = map(*x, *y);
                *x = nx;
                *y = ny;
            }
            OutlineCommand::QuadTo { cx, cy, x, y } => {
                let (ncx, ncy) = map(*cx, *cy);
                let (nx, ny) = map(*x, *y);
                *cx = ncx;
                *cy = ncy;
                *x = nx;
                *y = ny;
            }
            _ => {}
        }
    }
    outline.ink_bounds = bounds_of(&outline.commands);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push16(b: &mut Vec<u8>, v: u16) {
        b.extend_from_slice(&v.to_be_bytes());
    }
    fn push32(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_be_bytes());
    }

    /// A minimal format-4 cmap mapping `'A'` (U+0041) to glyph id 1, plus the
    /// mandatory `0xFFFF` terminator segment.
    fn cmap_a_to_1() -> Vec<u8> {
        let mut c = Vec::new();
        push16(&mut c, 0); // version
        push16(&mut c, 1); // numTables
        push16(&mut c, 3); // platform: Windows
        push16(&mut c, 1); // encoding: BMP
        push32(&mut c, 12); // subtable offset (right after this record)
                            // Format-4 subtable.
        push16(&mut c, 4); // format
        push16(&mut c, 32); // length
        push16(&mut c, 0); // language
        push16(&mut c, 4); // segCountX2 (2 segments)
        push16(&mut c, 0); // searchRange
        push16(&mut c, 0); // entrySelector
        push16(&mut c, 0); // rangeShift
        push16(&mut c, 0x41); // endCode[0]
        push16(&mut c, 0xFFFF); // endCode[1]
        push16(&mut c, 0); // reservedPad
        push16(&mut c, 0x41); // startCode[0]
        push16(&mut c, 0xFFFF); // startCode[1]
        push16(&mut c, 1u16.wrapping_sub(0x41)); // idDelta[0]: 0x41 -> 1
        push16(&mut c, 1); // idDelta[1]
        push16(&mut c, 0); // idRangeOffset[0]
        push16(&mut c, 0); // idRangeOffset[1]
        c
    }

    /// Assemble a minimal but valid two-glyph TrueType font (glyph 0 blank,
    /// glyph 1 = `glyf`) around the shared header/metric/cmap tables. `glyf`
    /// must have even length (short-loca half-offset constraint).
    fn synth_font(glyf: &[u8]) -> Arc<[u8]> {
        assert_eq!(glyf.len() % 2, 0, "glyf length must be even for short loca");

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
                                                              // indexToLocFormat (offset 50) stays 0 = short.

        let mut maxp = vec![0u8; 6];
        maxp[4..6].copy_from_slice(&2u16.to_be_bytes()); // numGlyphs

        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes()); // ascender
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes()); // descender
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes()); // numberOfHMetrics

        let mut hmtx = Vec::new();
        push16(&mut hmtx, 500); // gid0 advance
        push16(&mut hmtx, 0);
        push16(&mut hmtx, 600); // gid1 advance
        push16(&mut hmtx, 0);

        let mut loca = Vec::new();
        push16(&mut loca, 0); // gid0 start (empty)
        push16(&mut loca, 0); // gid1 start
        push16(&mut loca, (glyf.len() / 2) as u16); // end

        let tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cmap", cmap_a_to_1()),
            (*b"glyf", glyf.to_vec()),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ];

        let num = tables.len();
        let mut out = Vec::new();
        push32(&mut out, 0x0001_0000); // sfnt version
        push16(&mut out, num as u16);
        push16(&mut out, 0); // searchRange
        push16(&mut out, 0); // entrySelector
        push16(&mut out, 0); // rangeShift

        let mut offset = 12 + num * 16;
        let mut data = Vec::new();
        let mut records = Vec::new();
        for (tag, bytes) in &tables {
            records.push((*tag, offset as u32, bytes.len() as u32));
            data.extend_from_slice(bytes);
            offset += bytes.len();
        }
        for (tag, off, len) in records {
            out.extend_from_slice(&tag);
            push32(&mut out, 0); // checksum (unused)
            push32(&mut out, off);
            push32(&mut out, len);
        }
        out.extend_from_slice(&data);
        Arc::from(out.into_boxed_slice())
    }

    /// A simple triangular glyph: three on-curve points (0,0)->(100,0)->(0,100).
    fn triangle_glyf() -> Vec<u8> {
        let mut g = Vec::new();
        push16(&mut g, 1); // numberOfContours
        push16(&mut g, 0); // xMin
        push16(&mut g, 0); // yMin
        push16(&mut g, 100); // xMax
        push16(&mut g, 100); // yMax
        push16(&mut g, 2); // endPtsOfContours[0] (3 points: indices 0..=2)
        push16(&mut g, 0); // instructionLength
        g.push(0x01); // flags: on-curve
        g.push(0x01);
        g.push(0x01);
        // x deltas as i16: 0, +100, -100
        push16(&mut g, 0);
        push16(&mut g, 100);
        push16(&mut g, (-100i16) as u16);
        // y deltas as i16: 0, 0, +100
        push16(&mut g, 0);
        push16(&mut g, 0);
        push16(&mut g, 100);
        if g.len() % 2 != 0 {
            g.push(0);
        }
        g
    }

    #[test]
    fn synthetic_font_decodes_units_advance_and_cmap() {
        let face = SfntFace::parse(synth_font(&triangle_glyf()), 0).unwrap();
        assert_eq!(face.units_per_em(), 1000);
        assert_eq!(face.num_glyphs(), 2);
        assert_eq!(face.glyph_index('A'), Some(1));
        assert_eq!(face.glyph_index('Z'), None); // unmapped -> fallback boundary
        assert_eq!(face.advance_width(1), 600);
        assert_eq!(
            face.metrics(),
            SfntMetrics {
                units_per_em: 1000,
                ascender: 800,
                descender: -200,
                line_gap: 0,
            }
        );
    }

    #[test]
    fn simple_glyph_decodes_to_lines_and_matching_bounds() {
        let face = SfntFace::parse(synth_font(&triangle_glyf()), 0).unwrap();
        let outline = face.glyph_outline(1).unwrap();
        assert_eq!(outline.units_per_em, 1000);
        // First three points are on-curve, so the contour is all lines.
        assert!(outline
            .commands
            .iter()
            .all(|c| !matches!(c, OutlineCommand::QuadTo { .. })));
        assert!(matches!(
            outline.commands.first(),
            Some(OutlineCommand::MoveTo { .. })
        ));
        assert!(matches!(
            outline.commands.last(),
            Some(OutlineCommand::Close)
        ));
        // Bounds match the emitted points.
        assert_eq!(outline.ink_bounds.min_x, 0.0);
        assert_eq!(outline.ink_bounds.min_y, 0.0);
        assert_eq!(outline.ink_bounds.max_x, 100.0);
        assert_eq!(outline.ink_bounds.max_y, 100.0);
    }

    #[test]
    fn empty_glyph_resolves_to_blank_outline() {
        // gid 0 is blank (loca[0] == loca[1]); a blank glyph has no commands.
        let face = SfntFace::parse(synth_font(&triangle_glyf()), 0).unwrap();
        let outline = face.glyph_outline(0).unwrap();
        assert!(outline.commands.is_empty());
        assert_eq!(outline.ink_bounds, OutlineBounds::default());
    }

    #[test]
    fn composite_glyph_is_typed_rejection() {
        // numberOfContours = -1 marks a composite glyph.
        let mut glyf = Vec::new();
        push16(&mut glyf, (-1i16) as u16);
        push16(&mut glyf, 0); // xMin
        push16(&mut glyf, 0); // yMin
        push16(&mut glyf, 50); // xMax
        push16(&mut glyf, 50); // yMax
        let face = SfntFace::parse(synth_font(&glyf), 0).unwrap();
        assert_eq!(face.glyph_outline(1), Err(SfntError::CompositeGlyph));
    }

    #[test]
    fn glyph_out_of_range_is_typed_rejection() {
        let face = SfntFace::parse(synth_font(&triangle_glyf()), 0).unwrap();
        assert_eq!(face.glyph_outline(2), Err(SfntError::GlyphOutOfRange(2)));
    }

    #[test]
    fn cff_outlines_rejected_with_typed_error() {
        // `OTTO` sfnt version => CFF outlines, unsupported.
        let bytes: Arc<[u8]> =
            Arc::from(vec![0x4F, 0x54, 0x54, 0x4F, 0, 0, 0, 0].into_boxed_slice());
        assert_eq!(
            SfntFace::parse(bytes, 0).unwrap_err(),
            SfntError::UnsupportedOutlineFormat
        );
    }

    #[test]
    fn unknown_version_and_truncation_are_typed_rejections() {
        let unknown: Arc<[u8]> = Arc::from(vec![0u8; 8].into_boxed_slice());
        assert_eq!(
            SfntFace::parse(unknown, 0).unwrap_err(),
            SfntError::UnknownSfntVersion(0)
        );
        let truncated: Arc<[u8]> = Arc::from(vec![0u8; 3].into_boxed_slice());
        assert!(matches!(
            SfntFace::parse(truncated, 0),
            Err(SfntError::Malformed(_))
        ));
    }

    #[test]
    fn nonzero_face_index_on_single_font_is_rejected() {
        assert_eq!(
            SfntFace::parse(synth_font(&triangle_glyf()), 1).unwrap_err(),
            SfntError::FaceIndexOutOfRange(1)
        );
    }
}
