# CowPaper v0.3.0: historical Research Tag snapshots

Schema v21 adds `recommendation_items.tag_matches_snapshot_json`.

The value is the complete `TagMatch[]` produced by the analysis that created
or refreshed the current recommendation: each entry contains the displayed
tag label (`tag`), score, and persisted tag identity fields (`tagId` and
`semanticHash`) when available. It is self-contained and is never rebuilt
from `papers.tag_matches_json` or the live Research Tags table.

Rows created before v21 remain `NULL`; migration deliberately does not infer
historical explanations from mutable live analysis data. History uses
`score_snapshot`, rank, and this column only. A NULL snapshot means the old
historical explanation is unavailable and no live tag chips are shown.
