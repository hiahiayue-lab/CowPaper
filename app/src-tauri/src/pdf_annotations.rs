//! Read-only extraction of standard PDF annotations.
//!
//! This module deliberately has no writer, renderer, OCR, network, or AI
//! dependency. It reads the page /Annots dictionaries and uses lopdf's
//! bounded text/content APIs to recover text when a text layer and usable
//! geometry are available. The source PDF is never opened for writing.

use lopdf::{Document, Encoding, LoadOptions, Object, ObjectId};
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

#[derive(Debug, Clone)]
struct Glyph {
    text: String,
    x0: f32,
    x1: f32,
    y: f32,
    size: f32,
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

fn page_glyphs(doc: &Document, page_id: ObjectId) -> Vec<Glyph> {
    let Ok(fonts) = doc.get_page_fonts(page_id) else { return Vec::new() };
    let Ok(content) = doc.get_and_decode_page_content(page_id) else { return Vec::new() };
    let mut current_encoding: Option<Encoding<'_>> = None;
    let mut current_font_size = 12.0_f32;
    let mut x = 0.0_f32;
    let mut y = 0.0_f32;
    let mut leading = 0.0_f32;
    let mut glyphs = Vec::new();

    for operation in content.operations {
        match operation.operator.as_str() {
            "Tf" => {
                if let Some(font_name) = operation.operands.first().and_then(|value| value.as_name().ok()) {
                    current_encoding = fonts.get(font_name).and_then(|font| font.get_font_encoding(doc).ok());
                }
                current_font_size = operation.operands.get(1).and_then(|value| value.as_float().ok()).unwrap_or(current_font_size).abs().max(1.0);
            }
            "Tm" => {
                if operation.operands.len() >= 6 {
                    x = operation.operands[4].as_float().unwrap_or(x);
                    y = operation.operands[5].as_float().unwrap_or(y);
                }
            }
            "Td" | "TD" => {
                if operation.operands.len() >= 2 {
                    x += operation.operands[0].as_float().unwrap_or(0.0);
                    let dy = operation.operands[1].as_float().unwrap_or(0.0);
                    y += dy;
                    if operation.operator == "TD" { leading = -dy; }
                }
            }
            "T*" => y -= leading,
            "Tj" | "'" => {
                if operation.operator == "'" { y -= leading; }
                if let Some(value) = operation.operands.first() {
                    append_text_glyphs(&mut glyphs, current_encoding.as_ref(), value, &mut x, y, current_font_size);
                }
            }
            "\"" => {
                y -= leading;
                if let Some(value) = operation.operands.get(2) {
                    append_text_glyphs(&mut glyphs, current_encoding.as_ref(), value, &mut x, y, current_font_size);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = operation.operands.first() {
                    for item in items {
                        match item {
                            Object::String(_, _) => append_text_glyphs(&mut glyphs, current_encoding.as_ref(), item, &mut x, y, current_font_size),
                            Object::Integer(adjustment) => x -= (*adjustment as f32 / 1000.0) * current_font_size,
                            Object::Real(adjustment) => x -= (*adjustment / 1000.0) * current_font_size,
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

fn append_text_glyphs(
    glyphs: &mut Vec<Glyph>,
    encoding: Option<&Encoding<'_>>,
    value: &Object,
    x: &mut f32,
    y: f32,
    size: f32,
) {
    let Object::String(bytes, _) = value else { return };
    let Some(encoding) = encoding else { return };
    let Ok(text) = encoding.bytes_to_string(bytes) else { return };
    for ch in text.chars() {
        let advance = if ch.is_whitespace() { size * 0.28 } else if ch.is_ascii_punctuation() { size * 0.32 } else { size * 0.52 };
        glyphs.push(Glyph { text: ch.to_string(), x0: *x, x1: *x + advance, y, size });
        *x += advance;
    }
}

fn quote_for_quads(glyphs: &[Glyph], quads: &[Rect]) -> Option<String> {
    let mut pieces = Vec::new();
    for quad in quads {
        let mut selected: Vec<&Glyph> = glyphs.iter().filter(|glyph| {
            let center = (glyph.x0 + glyph.x1) / 2.0;
            center >= quad.x0 - glyph.size * 0.2
                && center <= quad.x1 + glyph.size * 0.2
                && glyph.y >= quad.y0 - glyph.size * 0.8
                && glyph.y <= quad.y1 + glyph.size * 0.8
        }).collect();
        selected.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let text = selected.into_iter().map(|glyph| glyph.text.as_str()).collect::<String>();
        if !text.trim().is_empty() { pieces.push(text); }
    }
    let quote = pieces.join(" ").trim().to_string();
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
