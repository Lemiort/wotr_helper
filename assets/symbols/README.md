Symbols templates
=================

Place one PNG template per symbol in this directory (transparent background recommended).

Filename conventions
- Use simple filenames matching keys used in `assets/symbols_map.json`, e.g. `swords.png`.
- Filenames should be lowercase and use underscores instead of spaces.

Mapping file (`assets/symbols_map.json`)
- JSON mapping of template filename → metadata. Example:

{
  "swords.png": {
    "name": "swords",
    "glyph": "⚔",
    "token": "SWORDS",
    "description": "Crossed-swords icon → Unicode U+2694 (⚔)"
  }
}

Notes and recommendations
- Templates should roughly match the scale and style of icons on your cards.
- Use simple, high-contrast templates for best results with template matching.
- Later, when symbol-detection is implemented, the script can load this mapping and record `glyph`/`token` for each detected symbol in `cards_summary.json`.
