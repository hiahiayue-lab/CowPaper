"""Minimal read-only PDF annotation extraction prototype.

Usage:
    python3 extract.py path/to/annotated.pdf

The prototype reads annotation dictionaries with pypdf and recovers text from
PDF text geometry with pdfplumber. It never writes to the input PDF.
"""

from __future__ import annotations

import argparse
import json
import re
import unicodedata
from pathlib import Path
from typing import Any

import pdfplumber
from pypdf import PdfReader


MARKUP_TYPES = {"/Highlight", "/Underline", "/StrikeOut"}
SUPPORTED = MARKUP_TYPES | {"/Text", "/FreeText"}
SUPPORTED_NAMES = {subtype.lstrip("/") for subtype in SUPPORTED}


def pdf_value(value: Any) -> Any:
    if value is None:
        return None
    if hasattr(value, "get_object"):
        value = value.get_object()
    if isinstance(value, (list, tuple)):
        return [pdf_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): pdf_value(item) for key, item in value.items()}
    return value


def normalize_for_match(text: str) -> str:
    text = unicodedata.normalize("NFKC", text)
    return re.sub(r"\s+", " ", text).strip().casefold()


def rect_from_quad(points: list[float]) -> tuple[float, float, float, float]:
    xs = points[0::2]
    ys = points[1::2]
    return min(xs), min(ys), max(xs), max(ys)


def overlap_ratio(char: dict[str, Any], rect: tuple[float, float, float, float], page_height: float) -> float:
    # pdfplumber reports top-origin coordinates; annotation coordinates are
    # bottom-origin default user space coordinates.
    cx0 = float(char["x0"])
    cx1 = float(char["x1"])
    cy0 = page_height - float(char["bottom"])
    cy1 = page_height - float(char["top"])
    rx0, ry0, rx1, ry1 = rect
    ix = max(0.0, min(cx1, rx1) - max(cx0, rx0))
    iy = max(0.0, min(cy1, ry1) - max(cy0, ry0))
    area = max(0.01, (cx1 - cx0) * (cy1 - cy0))
    return (ix * iy) / area


def extract_quad_text(page, quad_points: list[float]) -> str:
    rect = rect_from_quad(quad_points)
    chars = [
        char
        for char in page.chars
        if overlap_ratio(char, rect, float(page.height)) >= 0.15
    ]
    # This simple fixture algorithm handles horizontal Latin lines. The report
    # documents the production algorithm required for rotation and columns.
    chars.sort(key=lambda char: (round(float(char["top"]), 1), float(char["x0"])))
    pieces: list[str] = []
    previous = None
    for char in chars:
        if previous is not None:
            same_line = abs(float(char["top"]) - float(previous["top"])) < 2.0
            gap = float(char["x0"]) - float(previous["x1"])
            if same_line and gap > max(2.5, float(previous.get("size", 12)) * 0.35):
                pieces.append(" ")
        pieces.append(char.get("text", ""))
        previous = char
    return "".join(pieces).strip()


def annotation_to_record(page_number: int, raw: Any, plumber_page) -> dict[str, Any]:
    annot = raw.get_object() if hasattr(raw, "get_object") else raw
    subtype = str(annot.get("/Subtype", "/Unknown"))
    quad_points = pdf_value(annot.get("/QuadPoints"))
    if isinstance(quad_points, list):
        quad_points = [float(value) for value in quad_points]
    else:
        quad_points = None
    quoted = None
    if subtype in MARKUP_TYPES and quad_points and len(quad_points) % 8 == 0:
        fragments = []
        for start in range(0, len(quad_points), 8):
            fragments.append(extract_quad_text(plumber_page, quad_points[start : start + 8]))
        quoted = " ".join(fragment for fragment in fragments if fragment)
    contents = annot.get("/Contents")
    if contents is not None:
        contents = str(contents)
    return {
        "page": page_number,
        "annotation_type": subtype.lstrip("/"),
        "quadpoints": quad_points,
        "contents": contents,
        "quoted_text": quoted,
        "nm": str(annot["/NM"]) if annot.get("/NM") is not None else None,
        "author": str(annot["/T"]) if annot.get("/T") is not None else None,
        "modified": str(annot["/M"]) if annot.get("/M") is not None else None,
        "supported": subtype in SUPPORTED,
        "quoted_text_normalized": normalize_for_match(quoted) if quoted else None,
    }


def extract(path: Path) -> list[dict[str, Any]]:
    reader = PdfReader(str(path), strict=False)
    output: list[dict[str, Any]] = []
    with pdfplumber.open(str(path)) as plumber:
        for page_number, (pdf_page, plumber_page) in enumerate(zip(reader.pages, plumber.pages)):
            for raw in pdf_page.get("/Annots", []) or []:
                record = annotation_to_record(page_number, raw, plumber_page)
                if record["annotation_type"] in SUPPORTED_NAMES:
                    output.append(record)
    return output


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=Path)
    args = parser.parse_args()
    print(json.dumps(extract(args.pdf), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
