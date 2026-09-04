"""Create a synthetic, disposable PDF fixture for the annotation spike.

The fixture is intentionally self-contained and contains no user data. It is
used to validate that annotation dictionaries can be read without modifying
the source PDF.
"""

from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.annotations import FreeText, Highlight, Text
from pypdf.generic import ArrayObject, FloatObject, NameObject, TextStringObject
from reportlab.pdfbase.pdfmetrics import stringWidth
from reportlab.pdfgen import canvas


ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "research" / "pdf_annotations" / "fixtures"
BASE_PDF = OUT_DIR / "annotation_fixture_base.pdf"
FIXTURE_PDF = OUT_DIR / "annotation_fixture.pdf"
PAGE_W = 612.0
PAGE_H = 792.0


def quad(x0: float, y0: float, x1: float, y1: float) -> ArrayObject:
    # PDF text markup order is upper-left, upper-right, lower-left,
    # lower-right (the Z-shaped order used by PDF text markup annotations).
    return ArrayObject(
        [
            FloatObject(x0),
            FloatObject(y1),
            FloatObject(x1),
            FloatObject(y1),
            FloatObject(x0),
            FloatObject(y0),
            FloatObject(x1),
            FloatObject(y0),
        ]
    )


def add_markup(writer: PdfWriter, page, subtype: str, rect, quad_points, *, contents, name):
    annotation = Highlight(rect=rect, quad_points=quad_points, highlight_color="ffd966")
    annotation[NameObject("/Subtype")] = NameObject(f"/{subtype}")
    annotation[NameObject("/Contents")] = TextStringObject(contents)
    annotation[NameObject("/NM")] = TextStringObject(name)
    annotation[NameObject("/M")] = TextStringObject("D:20260903120000+08'00'")
    writer.add_annotation(page_number=0, annotation=annotation)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    # Base document: two lines with known coordinates. The second line is
    # represented by two QuadPoints fragments in a separate highlight below.
    c = canvas.Canvas(str(BASE_PDF), pagesize=(PAGE_W, PAGE_H))
    c.setTitle("CowPaper annotation extraction fixture")
    c.setFont("Helvetica-Bold", 16)
    c.drawString(54, 730, "PDF Annotation Extraction Fixture")
    c.setFont("Helvetica", 12)
    y1 = 670
    line1 = "Reliable quoted text comes from page geometry, not Contents."
    c.drawString(54, y1, line1)
    y2 = 635
    part_a = "Multiple highlight fragments can"
    part_b = " be joined in reading order."
    c.drawString(54, y2, part_a + part_b)
    c.setFont("Helvetica-Oblique", 10)
    c.drawString(54, 570, "The note below is deliberately independent from the marked text.")
    c.save()

    reader = PdfReader(str(BASE_PDF))
    writer = PdfWriter()
    writer.clone_document_from_reader(reader)
    page = writer.pages[0]

    line1_start = 54.0
    line1_end = line1_start + stringWidth(line1, "Helvetica", 12)
    add_markup(
        writer,
        page,
        "Highlight",
        (line1_start - 1, y1 - 2, line1_end + 1, y1 + 12),
        quad(line1_start, y1 - 1, line1_end, y1 + 11),
        contents="Reviewer comment is not the quoted text.",
        name="fixture-highlight-1",
    )

    part_a_start = 54.0
    part_a_end = part_a_start + stringWidth(part_a, "Helvetica", 12)
    part_b_start = part_a_end
    part_b_end = part_b_start + stringWidth(part_b, "Helvetica", 12)
    fragmented_quads = ArrayObject(
        list(quad(part_a_start, y2 - 1, part_a_end, y2 + 11))
        + list(quad(part_b_start, y2 - 1, part_b_end, y2 + 11))
    )
    add_markup(
        writer,
        page,
        "Highlight",
        (part_a_start - 1, y2 - 2, part_b_end + 1, y2 + 12),
        fragmented_quads,
        contents="Two geometric fragments",
        name="fixture-highlight-fragments",
    )
    add_markup(
        writer,
        page,
        "Underline",
        (part_a_start - 1, y2 - 2, part_a_end + 1, y2 + 12),
        quad(part_a_start, y2 - 1, part_a_end, y2 + 11),
        contents="",
        name="fixture-underline-1",
    )
    add_markup(
        writer,
        page,
        "StrikeOut",
        (part_b_start - 1, y2 - 2, part_b_end + 1, y2 + 12),
        quad(part_b_start, y2 - 1, part_b_end, y2 + 11),
        contents="Needs verification",
        name="fixture-strikeout-1",
    )

    text_note = Text(
        rect=(440, 520, 470, 550),
        text="Standalone text note",
        open=False,
        title_bar="Fixture Author",
    )
    text_note[NameObject("/NM")] = TextStringObject("fixture-text-1")
    text_note[NameObject("/M")] = TextStringObject("D:20260903120100+08'00'")
    writer.add_annotation(page_number=0, annotation=text_note)

    free_text = FreeText(
        rect=(54, 500, 300, 525),
        text="FreeText is visible directly on the page.",
        font="Helvetica",
        font_size="11pt",
        font_color="000000",
        background_color="e6f2ff",
        border_color="336699",
        title_bar="Fixture Author",
    )
    free_text[NameObject("/NM")] = TextStringObject("fixture-freetext-1")
    writer.add_annotation(page_number=0, annotation=free_text)

    with FIXTURE_PDF.open("wb") as stream:
        writer.write(stream)

    print(FIXTURE_PDF)


if __name__ == "__main__":
    main()
