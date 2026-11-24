### Changed

- A `TextContext` keeps the paragraphs it lays out a second time, keyed by every field of their `ParagraphSpec`, and a later layout of the same spec shares the kept layout instead of shaping it again, as Flutter's text does through Skia's `ParagraphCache`. A spec seen once costs its hash; the cache holds 128 paragraphs and about 512 KiB, and a font registration or `TextContext::reclaim` empties it.
