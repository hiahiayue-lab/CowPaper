//! Read-only extraction of standard PDF annotations.
//!
//! This module deliberately has no writer, renderer, OCR, network, or AI
//! dependency. It reads the page /Annots dictionaries and uses lopdf's
//! bounded text/content APIs to recover text when a text layer and usable
//! geometry are available. The source PDF is never opened for writing.

use lopdf::{Dictionary, Document, Encoding, LoadOptions, Object, ObjectId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

const MAX_DECOMPRESSED_PDF_BYTES: usize = 16 * 1024 * 1024;
const MARKUP_KINDS: &[&str] = &["Highlight", "Underline", "StrikeOut"];
const SUPPORTED_KINDS: &[&str] = &["Highlight", "Underline", "StrikeOut", "Text", "FreeText"];

#[derive(Debug, Clone)]
pub(crate) struct ExtractedAnnotation {
    pub external_annotation_id: Option<String>,
    pub kind: String,
    pub page_index: i64,
    pub color: Option<String>,
    pub quoted_text: Option<String>,
    pub comment: Option<String>,
    pub author: Option<String>,
    pub pdf_created_at: Option<String>,
    pub pdf_modified_at: Option<String>,
    pub fingerprint: String,
    pub extraction_status: String,
    pub raw_metadata_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfAnnotationScan {
    pub status: String,
    pub error: Option<String>,
    pub annotations: Vec<ExtractedAnnotation>,
    pub unsupported_count: i64,
}

/// One glyph placed in PDF *default user space* (the same space annotation
/// /Rect and /QuadPoints use, so page /Rotate never has to be re-applied here).
/// `certain` is false when the byte could not be mapped through an encoding we
/// trust: a quote that would contain such a glyph is refused rather than guessed.
#[derive(Debug, Clone)]
struct Glyph {
    text: String,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    size: f32,
    certain: bool,
    /// PDF text rendering mode. Mode 3 is invisible text: OCR layers and hidden
    /// duplicate text layers use it, so when a quad covers both a visible and an
    /// invisible layer the visible one wins.
    render_mode: u8,
    /// Which text-showing operation produced this glyph. Two runs whose x-ranges
    /// interleave on the same line mean the page's drawing order does not match
    /// its reading order (OCR text layers, multi-column overlays), and the quote
    /// geometry is then ambiguous.
    run: u32,
}

/// PDF text matrix, stored as the `[a b c d e f]` operands.
#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn translate(tx: f32, ty: f32) -> Matrix {
        Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    /// `self` first, then `outer` — the order `Td`/`T*` need when they post-
    /// multiply the current line matrix.
    fn then(self, outer: Matrix) -> Matrix {
        Matrix {
            a: self.a * outer.a + self.b * outer.c,
            b: self.a * outer.b + self.b * outer.d,
            c: self.c * outer.a + self.d * outer.c,
            d: self.c * outer.b + self.d * outer.d,
            e: self.e * outer.a + self.f * outer.c + outer.e,
            f: self.e * outer.b + self.f * outer.d + outer.f,
        }
    }

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

/// Glyph advance source, in unscaled text space (1/1000 em).
#[derive(Debug, Clone)]
enum Widths {
    Simple { first: i64, values: Vec<f32> },
    Cid { default: f32, ranges: Vec<(u32, u32, f32)> },
    Unknown,
}

impl Widths {
    fn width(&self, code: u32) -> Option<f32> {
        match self {
            Widths::Simple { first, values } => {
                let index = i64::from(code) - *first;
                if index < 0 { return None }
                values.get(index as usize).copied()
            }
            Widths::Cid { default, ranges } => ranges
                .iter()
                .find(|(start, end, _)| code >= *start && code <= *end)
                .map(|(_, _, width)| *width)
                .or(Some(*default)),
            Widths::Unknown => None,
        }
    }
}

struct PreparedFont<'a> {
    encoding: Encoding<'a>,
    two_byte: bool,
    widths: Widths,
    /// False when the code bytes cannot be mapped to text we trust. A two-byte
    /// (CID) font without a /ToUnicode CMap is the important case: its codes are
    /// glyph indices, so decoding them through a fallback one-byte encoding would
    /// silently invent Latin text. Such glyphs poison any quote that needs them.
    trusted: bool,
}

fn is_two_byte(font: &Dictionary) -> bool {
    let subtype = font
        .get(b"Subtype")
        .ok()
        .and_then(|value| value.as_name().ok())
        .map(|name| String::from_utf8_lossy(name).to_string())
        .unwrap_or_default();
    subtype == "Type0" || font.get(b"DescendantFonts").is_ok()
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

pub(crate) fn scan_path(path: &Path) -> PdfAnnotationScan {
    let doc = match Document::load_with_options(
        path,
        LoadOptions::with_max_decompressed_size(MAX_DECOMPRESSED_PDF_BYTES),
    ) {
        Ok(doc) => doc,
        Err(error) => {
            return PdfAnnotationScan {
                status: "malformed".into(),
                error: Some(error.to_string()),
                annotations: Vec::new(),
                unsupported_count: 0,
            }
        }
    };

    let pages = doc.get_pages();
    let mut extracted = Vec::new();
    let mut unsupported_count = 0_i64;
    for (page_number, page_id) in pages {
        let entries = page_annotation_entries(&doc, page_id);
        let glyphs = page_glyphs(&doc, page_id);
        let page_has_text = !glyphs.is_empty();
        for (object_id, annotation) in entries {
            let Some(kind) = object_name(&doc, annotation.get(b"Subtype").ok()) else {
                unsupported_count += 1;
                continue;
            };
            if !SUPPORTED_KINDS.contains(&kind.as_str()) {
                unsupported_count += 1;
                continue;
            }

            let external_id = object_text(&doc, annotation.get(b"NM").ok());
            let comment = object_text(&doc, annotation.get(b"Contents").ok());
            let author = object_text(&doc, annotation.get(b"T").ok());
            let pdf_modified_at = object_text(&doc, annotation.get(b"M").ok());
            let pdf_created_at = object_text(&doc, annotation.get(b"CreationDate").ok());
            let color = color_value(&doc, annotation.get(b"C").ok());
            let rect = number_array(&doc, annotation.get(b"Rect").ok());
            let quad_points = number_array(&doc, annotation.get(b"QuadPoints").ok());
            let quads = quad_points.as_ref().and_then(|points| {
                if points.len() % 8 != 0 {
                    None
                } else {
                    Some(points.chunks_exact(8).filter_map(rect_from_quad).collect::<Vec<_>>())
                }
            });
            let malformed_quads = MARKUP_KINDS.contains(&kind.as_str())
                && quad_points.as_ref().is_some_and(|points| points.is_empty() || points.len() % 8 != 0);
            let quoted_text = if MARKUP_KINDS.contains(&kind.as_str()) {
                quads.as_ref().and_then(|items| quote_for_quads(&glyphs, items))
            } else {
                None
            };
            let extraction_status = if malformed_quads {
                "malformed"
            } else if MARKUP_KINDS.contains(&kind.as_str()) && quoted_text.is_none() {
                if page_has_text { "no_text" } else { "scanned" }
            } else {
                "extracted"
            };
            let raw_metadata = json!({
                "objectId": object_id.map(|(number, generation)| json!([number, generation])),
                "rect": rect,
                "quadPoints": quad_points,
                "color": color,
                "popup": reference_value(annotation.get(b"Popup").ok()),
                "inReplyTo": reference_value(annotation.get(b"IRT").ok()),
                "subtype": kind,
            });
            let identity_material = if let Some(external_id) = external_id.as_deref() {
                format!("nm\n{}\n{}\n{}", page_number - 1, kind, external_id)
            } else {
                format!(
                    "fallback\n{}\n{}\n{}\n{}\n{}",
                    page_number - 1,
                    kind,
                    canonical_numbers(rect.as_deref()),
                    canonical_numbers(quad_points.as_deref()),
                    normalize_text(quoted_text.as_deref().or(comment.as_deref()).unwrap_or("")),
                )
            };
            extracted.push(ExtractedAnnotation {
                external_annotation_id: external_id,
                kind,
                page_index: i64::from(page_number.saturating_sub(1)),
                color,
                quoted_text,
                comment,
                author,
                pdf_created_at,
                pdf_modified_at,
                fingerprint: fingerprint(&identity_material),
                extraction_status: extraction_status.into(),
                raw_metadata_json: raw_metadata.to_string(),
            });
        }
    }

    // NM is only required to be unique within a page. Do not merge two
    // malformed/repeated entries under one external id.
    let mut counts: HashMap<(i64, String, String), usize> = HashMap::new();
    for item in &extracted {
        if let Some(external_id) = item.external_annotation_id.as_ref() {
            *counts
                .entry((item.page_index, item.kind.clone(), external_id.clone()))
                .or_default() += 1;
        }
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &mut extracted {
        let duplicate_nm = item.external_annotation_id.as_ref().is_some_and(|external_id| {
            counts.get(&(item.page_index, item.kind.clone(), external_id.clone())).copied().unwrap_or(0) > 1
        });
        if duplicate_nm || seen.insert(item.fingerprint.clone(), 1).is_some() {
            let fallback = format!(
                "duplicate-fallback\n{}\n{}\n{}\n{}",
                item.page_index,
                item.kind,
                item.raw_metadata_json,
                normalize_text(item.quoted_text.as_deref().or(item.comment.as_deref()).unwrap_or("")),
            );
            item.fingerprint = fingerprint(&fallback);
            let ordinal = seen.entry(item.fingerprint.clone()).or_default();
            *ordinal += 1;
            if *ordinal > 1 {
                item.fingerprint = fingerprint(&format!("{}\n{}", fallback, ordinal));
            }
        }
    }

    PdfAnnotationScan {
        status: "completed".into(),
        error: None,
        annotations: extracted,
        unsupported_count,
    }
}

fn page_annotation_entries(doc: &Document, page_id: ObjectId) -> Vec<(Option<ObjectId>, lopdf::Dictionary)> {
    let Ok(page) = doc.get_dictionary(page_id) else { return Vec::new() };
    let Ok(value) = page.get(b"Annots") else { return Vec::new() };
    let Some(array) = dereferenced(doc, value).and_then(|object| object.as_array().ok()) else {
        return Vec::new();
    };
    array.iter().filter_map(|item| {
        let object_id = item.as_reference().ok();
        let dictionary = dereferenced(doc, item).and_then(|object| object.as_dict().ok())?.clone();
        Some((object_id, dictionary))
    }).collect()
}

fn dereferenced<'a>(doc: &'a Document, value: &'a Object) -> Option<&'a Object> {
    doc.dereference(value).ok().map(|(_, object)| object)
}

fn object_name(doc: &Document, value: Option<&Object>) -> Option<String> {
    let value = dereferenced(doc, value?)?;
    value.as_name().ok().map(|name| String::from_utf8_lossy(name).into_owned())
}

fn object_text(doc: &Document, value: Option<&Object>) -> Option<String> {
    let value = dereferenced(doc, value?)?;
    match value {
        Object::String(_, _) => lopdf::decode_text_string(value).ok().and_then(|text| clean_text(&text)),
        Object::Name(bytes) => clean_text(&String::from_utf8_lossy(bytes)),
        _ => None,
    }
}

fn clean_text(value: &str) -> Option<String> {
    let value = value.replace('\0', "").trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn number_array(doc: &Document, value: Option<&Object>) -> Option<Vec<f32>> {
    let value = dereferenced(doc, value?)?;
    let array = value.as_array().ok()?;
    let mut numbers = Vec::with_capacity(array.len());
    for item in array {
        numbers.push(dereferenced(doc, item)?.as_float().ok()?);
    }
    Some(numbers)
}

fn rect_from_quad(points: &[f32]) -> Option<Rect> {
    if points.len() != 8 { return None }
    let xs = [points[0], points[2], points[4], points[6]];
    let ys = [points[1], points[3], points[5], points[7]];
    Some(Rect {
        x0: xs.into_iter().fold(f32::INFINITY, f32::min),
        y0: ys.into_iter().fold(f32::INFINITY, f32::min),
        x1: xs.into_iter().fold(f32::NEG_INFINITY, f32::max),
        y1: ys.into_iter().fold(f32::NEG_INFINITY, f32::max),
    })
}

fn color_value(doc: &Document, value: Option<&Object>) -> Option<String> {
    let values = number_array(doc, value)?;
    if values.len() < 3 { return None }
    let rgb = values.iter().take(3).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect::<Vec<_>>();
    Some(format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

fn reference_value(value: Option<&Object>) -> Value {
    match value {
        Some(Object::Reference((number, generation))) => json!([number, generation]),
        Some(Object::Array(items)) => json!(items.iter().map(|item| reference_value(Some(item))).collect::<Vec<_>>()),
        Some(Object::Name(name)) => json!(String::from_utf8_lossy(name)),
        Some(Object::String(bytes, _)) => json!(String::from_utf8_lossy(bytes)),
        Some(Object::Integer(value)) => json!(value),
        Some(Object::Real(value)) => json!(value),
        _ => Value::Null,
    }
}

fn canonical_numbers(values: Option<&[f32]>) -> String {
    values.map(|items| items.iter().map(|value| format!("{value:.3}")).collect::<Vec<_>>().join(",")).unwrap_or_default()
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn fingerprint(value: &str) -> String {
    format!("v1-{:x}", Sha256::digest(value.as_bytes()))
}

fn read_widths(doc: &Document, font: &Dictionary, two_byte: bool) -> Widths {
    if two_byte {
        let mut default = 1000.0_f32;
        let mut ranges: Vec<(u32, u32, f32)> = Vec::new();
        let descendant = font
            .get_deref(b"DescendantFonts", doc)
            .ok()
            .and_then(|value| value.as_array().ok())
            .and_then(|items| items.first())
            .and_then(|first| doc.dereference(first).ok().map(|(_, object)| object))
            .and_then(|object| object.as_dict().ok());
        if let Some(descendant) = descendant {
            if let Some(value) = descendant.get(b"DW").ok().and_then(|value| value.as_float().ok()) {
                default = value;
            }
            if let Some(list) = descendant.get(b"W").ok().and_then(|value| value.as_array().ok()) {
                let mut index = 0;
                while index < list.len() {
                    let Some(start) = list[index].as_float().ok() else { break };
                    let start = start.max(0.0) as u32;
                    match list.get(index + 1) {
                        Some(Object::Array(widths)) => {
                            for (offset, width) in widths.iter().enumerate() {
                                if let Some(width) = width.as_float().ok() {
                                    let code = start.saturating_add(offset as u32);
                                    ranges.push((code, code, width));
                                }
                            }
                            index += 2;
                        }
                        Some(end) => {
                            let (Some(end), Some(width)) = (
                                end.as_float().ok(),
                                list.get(index + 2).and_then(|value| value.as_float().ok()),
                            ) else {
                                break;
                            };
                            ranges.push((start, end.max(start as f32) as u32, width));
                            index += 3;
                        }
                        None => break,
                    }
                }
            }
        }
        return Widths::Cid { default, ranges };
    }
    let first = font
        .get(b"FirstChar")
        .ok()
        .and_then(|value| value.as_float().ok())
        .map(|value| value as i64)
        .unwrap_or(0);
    match font.get(b"Widths").ok().and_then(|value| value.as_array().ok()) {
        Some(items) => Widths::Simple {
            first,
            values: items.iter().filter_map(|item| item.as_float().ok()).collect(),
        },
        None => Widths::Unknown,
    }
}

/// Walk the page content stream, maintaining the real PDF text state (line
/// matrix, text matrix, leading, character/word spacing, horizontal scale and
/// rise) so every glyph lands at its true user-space position. Fallback metrics
/// are deliberately nominal: a wrong width can only shift a run's tail, and the
/// selection step refuses partial coverage instead of emitting a truncated quote.
/// Test-only probe: how many glyphs the content-stream walk finds for a page.
/// Test-only probe: the raw text-positioning operators near the start of a page.
#[cfg(test)]
pub(crate) fn diagnostic_content_ops(doc: &Document, page_id: ObjectId, limit: usize) -> Vec<String> {
    let Ok(content) = doc.get_and_decode_page_content(page_id) else { return Vec::new() };
    let mut out = Vec::new();
    let mut do_count = 0;
    for operation in &content.operations {
        if operation.operator == "Do" {
            do_count += 1;
        }
        if !matches!(operation.operator.as_str(), "BT" | "ET" | "Tm" | "Td" | "TD" | "T*" | "Tf" | "Tj" | "TJ" | "TL" | "Tr" | "Do") {
            continue;
        }
        if out.len() >= limit {
            break;
        }
        let operands = operation
            .operands
            .iter()
            .map(|value| match value {
                Object::Real(number) => format!("{number:.2}"),
                Object::Integer(number) => number.to_string(),
                Object::Name(name) => String::from_utf8_lossy(name).to_string(),
                Object::String(bytes, _) => format!("<{} bytes>", bytes.len()),
                Object::Array(items) => format!("[{}]", items.len()),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push(format!("{} {}", operation.operator, operands));
    }
    out.push(format!("TOTAL_Do={do_count}"));
    out
}

#[cfg(test)]
pub(crate) fn diagnostic_page_glyph_count(doc: &Document, page_id: ObjectId) -> (usize, usize, usize) {
    let glyphs = page_glyphs(doc, page_id);
    let certain = glyphs.iter().filter(|glyph| glyph.certain).count();
    let distinct_runs = glyphs.iter().map(|glyph| glyph.run).max().unwrap_or(0) as usize;
    (glyphs.len(), certain, distinct_runs)
}

fn page_glyphs(doc: &Document, page_id: ObjectId) -> Vec<Glyph> {
    let Ok(fonts) = doc.get_page_fonts(page_id) else { return Vec::new() };
    let Ok(content) = doc.get_and_decode_page_content(page_id) else { return Vec::new() };
    // A resolved Encoding may borrow its font dictionary, so every resource
    // first gets one prepared copy in storage that outlives the table; the
    // encodings are resolved only after storage is complete.
    let mut storage: Vec<Dictionary> = fonts
        .values()
        .map(|font| {
            let mut prepared = (*font).clone();
            // `/Encoding` on a CID font is a predefined CMap *name*, not a byte
            // encoding: drop it so the /ToUnicode CMap is resolved instead.
            if is_two_byte(font) && prepared.get(b"ToUnicode").is_ok() {
                prepared.remove(b"Encoding");
            }
            prepared
        })
        .collect();
    storage.shrink_to_fit();
    let mut table: HashMap<Vec<u8>, Option<PreparedFont<'_>>> = HashMap::new();
    for (index, (name, font)) in fonts.iter().enumerate() {
        let two_byte = is_two_byte(font);
        let prepared = &storage[index];
        let encoding = prepared.get_font_encoding(doc).ok();
        let widths = read_widths(doc, prepared, two_byte);
        let trusted = match (&encoding, two_byte) {
            (Some(Encoding::UnicodeMapEncoding(_)), _) => true,
            (Some(_), false) => true,
            _ => false,
        };
        table.insert(
            name.clone(),
            encoding.map(|encoding| PreparedFont { encoding, two_byte, widths, trusted }),
        );
    }
    // Encoding is not Clone, so the current font is remembered by resource name
    // and resolved from the table when text is actually shown.
    let mut font_name: Option<Vec<u8>> = None;
    let font_for = |name: &Option<Vec<u8>>| -> Option<&PreparedFont<'_>> {
        name.as_ref().and_then(|name| table.get(name)).and_then(|entry| entry.as_ref())
    };
    let mut size = 12.0_f32;
    let mut char_spacing = 0.0_f32;
    let mut word_spacing = 0.0_f32;
    let mut h_scale = 1.0_f32;
    let mut leading = 0.0_f32;
    let mut rise = 0.0_f32;
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;
    let mut pen = 0.0_f32;
    let mut run = 0_u32;
    let mut render_mode = 0_u8;
    let mut glyphs = Vec::new();

    let mut next_line = |tlm: &mut Matrix, tm: &mut Matrix, pen: &mut f32, tx: f32, ty: f32| {
        *tlm = tlm.then(Matrix::translate(tx, ty));
        *tm = *tlm;
        *pen = 0.0;
    };

    for operation in content.operations {
        let operands = &operation.operands;
        let number = |index: usize| operands.get(index).and_then(|value| value.as_float().ok());
        match operation.operator.as_str() {
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
                pen = 0.0;
            }
            "Tf" => {
                font_name = operands.first().and_then(|value| value.as_name().ok()).map(|name| name.to_vec());
                if let Some(value) = number(1) {
                    size = value.abs().max(0.1);
                }
            }
            "TL" => leading = number(0).unwrap_or(leading),
            "Tc" => char_spacing = number(0).unwrap_or(0.0),
            "Tw" => word_spacing = number(0).unwrap_or(0.0),
            "Tz" => h_scale = number(0).unwrap_or(100.0) / 100.0,
            "Ts" => rise = number(0).unwrap_or(0.0),
            "Tr" => render_mode = number(0).unwrap_or(0.0).clamp(0.0, 7.0) as u8,
            "Tm" => {
                let values = operands.iter().filter_map(|value| value.as_float().ok()).collect::<Vec<_>>();
                if values.len() == 6 {
                    tm = Matrix { a: values[0], b: values[1], c: values[2], d: values[3], e: values[4], f: values[5] };
                    tlm = tm;
                    pen = 0.0;
                }
            }
            "Td" | "TD" => {
                let (tx, ty) = (number(0).unwrap_or(0.0), number(1).unwrap_or(0.0));
                if operation.operator == "TD" {
                    leading = -ty;
                }
                next_line(&mut tlm, &mut tm, &mut pen, tx, ty);
            }
            "T*" => next_line(&mut tlm, &mut tm, &mut pen, 0.0, -leading),
            "Tj" => show_text(&mut glyphs, font_for(&font_name), operands.first(), size, tm, char_spacing, word_spacing, h_scale, rise, &mut pen, &mut run, render_mode),
            "'" => {
                next_line(&mut tlm, &mut tm, &mut pen, 0.0, -leading);
                show_text(&mut glyphs, font_for(&font_name), operands.first(), size, tm, char_spacing, word_spacing, h_scale, rise, &mut pen, &mut run, render_mode);
            }
            "\"" => {
                if let Some(value) = number(0) {
                    word_spacing = value;
                }
                if let Some(value) = number(1) {
                    char_spacing = value;
                }
                next_line(&mut tlm, &mut tm, &mut pen, 0.0, -leading);
                show_text(&mut glyphs, font_for(&font_name), operands.get(2), size, tm, char_spacing, word_spacing, h_scale, rise, &mut pen, &mut run, render_mode);
            }
            "TJ" => {
                if let Some(Object::Array(items)) = operands.first() {
                    for item in items {
                        match item {
                            Object::String(_, _) => show_text(&mut glyphs, font_for(&font_name), Some(item), size, tm, char_spacing, word_spacing, h_scale, rise, &mut pen, &mut run, render_mode),
                            Object::Integer(adjustment) => pen -= *adjustment as f32,
                            Object::Real(adjustment) => pen -= *adjustment,
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    glyphs
}

/// Decode one shown string into positioned glyphs, advancing the text-space pen.
#[allow(clippy::too_many_arguments)]
fn show_text(
    glyphs: &mut Vec<Glyph>,
    font_state: Option<&PreparedFont<'_>>,
    value: Option<&Object>,
    size: f32,
    tm: Matrix,
    char_spacing: f32,
    word_spacing: f32,
    h_scale: f32,
    rise: f32,
    pen: &mut f32,
    run: &mut u32,
    render_mode: u8,
) {
    let (Some(font), Some(Object::String(bytes, _))) = (font_state, value) else { return };
    *run += 1;
    let codes = split_codes(bytes, font.two_byte);
    for (code, raw) in codes {
        // A code we cannot decode is never invented: it is carried through as an
        // uncertain glyph so the caller can refuse the whole quote.
        let decoded = font.encoding.bytes_to_string(raw).ok().filter(|text| !text.contains('\u{FFFD}'));
        let certain = font.trusted && decoded.is_some();
        let text = decoded.unwrap_or_else(|| "\u{FFFD}".to_string());
        let width = font.widths.width(code).unwrap_or(if text.chars().all(char::is_whitespace) { 250.0 } else { 500.0 });
        let advance = width + char_spacing + if text.chars().all(char::is_whitespace) { word_spacing } else { 0.0 };
        let (x0, y0, x1, y1) = glyph_box(tm, *pen, *pen + width, size, h_scale, rise);
        glyphs.push(Glyph { text, x0, y0, x1, y1, size, certain, render_mode, run: *run });
        *pen += advance;
    }
}

/// Split a shown string into (code, bytes) pairs. Two-byte (CID) fonts use
/// 2-byte codes; simple fonts use single bytes.
fn split_codes(bytes: &[u8], two_byte: bool) -> Vec<(u32, &[u8])> {
    if two_byte {
        bytes
            .chunks(2)
            .map(|chunk| {
                let code = chunk.iter().fold(0_u32, |acc, byte| acc * 256 + u32::from(*byte));
                (code, chunk)
            })
            .collect()
    } else {
        bytes.iter().map(|byte| (u32::from(*byte), std::slice::from_ref(byte))).collect()
    }
}

/// Axis-aligned user-space box for a glyph occupying `[from, to]` in unscaled
/// text space (1/1000 em). The vertical band approximates ascender/descender
/// around the baseline; it is only used for selection, never written out.
fn glyph_box(tm: Matrix, from: f32, to: f32, size: f32, h_scale: f32, rise: f32) -> (f32, f32, f32, f32) {
    let scale_x = size * h_scale / 1000.0;
    let scale_y = size / 1000.0;
    let corners = [(from, -220.0), (from, 780.0), (to, -220.0), (to, 780.0)];
    let mapped = corners.map(|(x, y)| tm.apply(x * scale_x, y * scale_y + rise));
    let xs = mapped.map(|(x, _)| x);
    let ys = mapped.map(|(_, y)| y);
    (
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    )
}

/// Two text runs whose x-ranges overlap on the same baseline make the reading
/// order ambiguous: the page draws them interleaved (typical of OCR text layers
/// or overlaid columns), so geometry alone cannot order the characters.
fn runs_interleave(selected: &[&Glyph]) -> bool {
    let mut runs: Vec<(u32, f32, f32, f32, f32)> = Vec::new();
    for glyph in selected {
        match runs.iter_mut().find(|(run, ..)| *run == glyph.run) {
            Some(entry) => {
                entry.1 = entry.1.min(glyph.x0);
                entry.2 = entry.2.max(glyph.x1);
                entry.3 = entry.3.min(glyph.y0);
                entry.4 = entry.4.max(glyph.y1);
            }
            None => runs.push((glyph.run, glyph.x0, glyph.x1, glyph.y0, glyph.y1)),
        }
    }
    for (index, left) in runs.iter().enumerate() {
        for right in runs.iter().skip(index + 1) {
            let vertical = left.4.min(right.4) - left.3.max(right.3);
            if vertical <= 0.0 {
                continue;
            }
            let overlap = left.2.min(right.2) - left.1.max(right.1);
            let narrowest = (left.2 - left.1).min(right.2 - right.1).max(0.001);
            if overlap > narrowest * 0.2 {
                return true;
            }
        }
    }
    false
}

fn glyph_in_quad(glyph: &Glyph, quad: &Rect) -> bool {
    let height = (glyph.y1 - glyph.y0).max(0.001);
    let centre = (glyph.x0 + glyph.x1) / 2.0;
    let overlap = glyph.y1.min(quad.y1) - glyph.y0.max(quad.y0);
    let tolerance = glyph.size.max(1.0) * 0.25;
    centre >= quad.x0 - tolerance && centre <= quad.x1 + tolerance && overlap >= height * 0.4
}

/// Recover the page text that a set of annotation quads actually covers.
///
/// Conservative by construction: glyphs come from real positions and real
/// widths, the selection must cover at least half of the quad, and any glyph
/// whose code could not be decoded poisons the whole quote into `None`. The
/// annotation `/Contents` is never used as a substitute.
fn quote_for_quads(glyphs: &[Glyph], quads: &[Rect]) -> Option<String> {
    let mut fragments: Vec<(f32, f32, String)> = Vec::new();
    for quad in quads {
        let mut selected = glyphs.iter().filter(|glyph| glyph_in_quad(glyph, quad)).collect::<Vec<_>>();
        if selected.is_empty() {
            continue;
        }
        // A page may carry a hidden duplicate text layer (invisible Tr 3) on top
        // of the real one. When both are inside the quad, the visible layer is
        // what the user highlighted; an OCR-only page keeps its invisible layer.
        if selected.iter().any(|glyph| glyph.render_mode != 3) {
            selected.retain(|glyph| glyph.render_mode != 3);
        }
        if selected.iter().any(|glyph| !glyph.certain) {
            return None;
        }
        if runs_interleave(&selected) {
            // The drawing order does not match the reading order here, so any
            // reconstruction would be a guess. Refuse instead of inventing text.
            return None;
        }
        selected.sort_by(|a, b| {
            let line = (a.y0 - b.y0).abs();
            if line > a.size.max(b.size).max(1.0) * 0.5 {
                b.y0.total_cmp(&a.y0)
            } else {
                a.x0.total_cmp(&b.x0)
            }
        });
        let mut text = String::new();
        let mut previous: Option<&Glyph> = None;
        for glyph in &selected {
            if let Some(previous) = previous {
                let new_line = (glyph.y0 - previous.y0).abs() > previous.size.max(1.0) * 0.5;
                let gap = glyph.x0 - previous.x1;
                if (new_line || gap > previous.size.max(1.0) * 0.22) && !text.ends_with(' ') {
                    text.push(' ');
                }
            }
            text.push_str(&glyph.text);
            previous = Some(glyph);
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let left = selected.iter().map(|glyph| glyph.x0).fold(f32::INFINITY, f32::min);
        let top = selected.iter().map(|glyph| glyph.y1).fold(f32::NEG_INFINITY, f32::max);
        // The quad must actually be *filled* with text we could type. Summing the
        // glyph advances (rather than their outer span) rejects a sparse selection
        // where a couple of stray glyphs straddle a wide quad — the signature of an
        // imprecise text layer — instead of emitting a fragment as a quote.
        let typed: f32 = selected.iter().map(|glyph| (glyph.x1 - glyph.x0).max(0.0)).sum();
        if typed < (quad.x1 - quad.x0).max(0.001) * 0.5 {
            return None;
        }
        fragments.push((top, left, text));
    }
    if fragments.is_empty() {
        return None;
    }
    fragments.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
    let quote = fragments.into_iter().map(|(_, _, text)| text).collect::<Vec<_>>().join(" ").trim().to_string();
    (!quote.is_empty()).then_some(quote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_text_and_fingerprints_deterministically() {
        assert_eq!(normalize_text("  A\nB  "), "a b");
        assert_eq!(fingerprint("same"), fingerprint("same"));
        assert_ne!(fingerprint("same"), fingerprint("different"));
    }

    #[test]
    fn quad_rect_accepts_z_order_and_rejects_bad_length() {
        let rect = rect_from_quad(&[10.0, 30.0, 50.0, 30.0, 10.0, 10.0, 50.0, 10.0]).unwrap();
        assert_eq!((rect.x0, rect.y0, rect.x1, rect.y1), (10.0, 10.0, 50.0, 30.0));
        assert!(rect_from_quad(&[1.0, 2.0]).is_none());
    }
}
