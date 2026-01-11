#!/usr/bin/env python3
"""
parse_regions.py

Usage examples:
  - Process atlas using regions JSON (uses image_size in regions file when present):
      python scripts/parse_regions.py --atlas assets/light_cards.png --regions assets/card2.json --out out --ocr

  - If regions JSON lacks image_size, provide card size:
      python scripts/parse_regions.py --atlas assets/light_cards.png --regions assets/card2.json --card-width 535 --card-height 752 --out out --ocr

  - Enable symbol/template detection (requires templates in --symbols-dir and a mapping file):
      python scripts/parse_regions.py --atlas assets/light_cards.png --regions assets/card2.json --out out --ocr --symbols-dir assets/symbols --symbols-map assets/symbols_map.json --symbol-threshold 0.65

  - Save individual crops and OCR text files (disabled by default; use for debugging):
      python scripts/parse_regions.py ... --save-crops

What it does:
- Loads a regions JSON saved by the app (supports both the new object with `image_size` + `regions` and the old plain `Vec<Region>`).
- Tiles the atlas into cards (using card width/height) and iterates every card.
- For each region (coordinates are relative to a single card), crops in-memory and optionally runs OCR.
- Optionally runs symbol/template matching (if `--symbols-dir` and `--symbols-map` are provided) and **merges detected symbols into the region `text` field** as Unicode glyphs (no separate `symbols` array).
- Writes a summary JSON: `out/cards_summary.json` containing atlas/card metadata and per-card region entries (including `card_index` and any `symbols` detected).
- If `--save-crops` is set, saves cropped PNGs and OCR .txt files to the output directory.

Dependencies:
- pillow (PIL)
- pytesseract (optional; requires Tesseract OCR installed on the system)
- opencv-python (optional; required for symbol/template matching)

Install deps:
    pip install pillow pytesseract
    pip install opencv-python  # for symbol detection

On Windows you must also install Tesseract (https://github.com/tesseract-ocr/tesseract). On Linux: `sudo apt install tesseract-ocr`.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import List, Dict, Any, Optional, Tuple

from PIL import Image

try:
    import pytesseract  # type: ignore
    HAS_PYTESSERACT = True
except Exception:
    HAS_PYTESSERACT = False

try:
    import cv2  # type: ignore
    import numpy as np  # type: ignore
    HAS_CV2 = True
except Exception:
    HAS_CV2 = False


def load_regions(path: Path) -> Tuple[List[Dict[str, Any]], Optional[Tuple[int, int]]]: 
    s = path.read_text(encoding="utf-8")
    try:
        obj = json.loads(s)
    except Exception as e:
        raise RuntimeError(f"Failed to parse JSON: {e}") from e

    # New format: { "image_size": [w,h], "regions": [ ... ] }
    if isinstance(obj, dict) and "regions" in obj and isinstance(obj["regions"], list):
        image_size = None
        if "image_size" in obj and isinstance(obj["image_size"], list) and len(obj["image_size"]) >= 2:
            try:
                image_size = (int(obj["image_size"][0]), int(obj["image_size"][1]))
            except Exception:
                image_size = None
        return obj["regions"], image_size

    # Old format: just a list of regions (no image_size)
    if isinstance(obj, list):
        return obj, None

    raise RuntimeError("Unrecognized regions JSON format")


def process_atlas(atlas_path: Path, regions: List[Dict[str, Any]], image_size: Optional[Tuple[int, int]], out_dir: Path, do_ocr: bool, save_crops: bool, card_w_arg: Optional[int], card_h_arg: Optional[int], symbols_dir: Optional[Path] = None, symbols_map_path: Optional[Path] = None, symbol_threshold: float = 0.6, symbol_edge_threshold: float = 0.2) -> None:
    img = Image.open(atlas_path).convert("RGBA")
    atlas_w, atlas_h = img.size
    out_dir.mkdir(parents=True, exist_ok=True)

    # Determine card size (width, height)
    if image_size is not None:
        card_w, card_h = image_size
    elif card_w_arg and card_h_arg:
        card_w, card_h = card_w_arg, card_h_arg
    else:
        raise RuntimeError("Card size not found in regions JSON; provide --card-width and --card-height")

    cols = atlas_w // card_w
    rows = atlas_h // card_h
    if cols <= 0 or rows <= 0:
        raise RuntimeError(f"Card size {card_w}x{card_h} is larger than atlas {atlas_w}x{atlas_h}")

    if do_ocr and not HAS_PYTESSERACT:
        print("pytesseract not available; skipping OCR. Install via `pip install pytesseract` and ensure Tesseract binary is on PATH.")
        do_ocr = False

    # Prepare templates if symbol detection requested
    templates = []  # list of dicts: {"filename", "name", "glyph", "token", "tpl"}
    if symbols_dir and symbols_map_path:
        if not HAS_CV2:
            print("OpenCV not available; skipping symbol detection. Install via `pip install opencv-python`")
        else:
            try:
                sym_map = json.loads(symbols_map_path.read_text(encoding="utf-8"))
            except Exception as e:
                print(f"Failed to load symbols map {symbols_map_path}: {e}")
                sym_map = {}

            for filename, meta in sym_map.items():
                tpl_path = Path(symbols_dir) / filename
                if not tpl_path.exists():
                    print(f"Template not found: {tpl_path}; skipping")
                    continue
                tpl = cv2.imread(str(tpl_path), cv2.IMREAD_GRAYSCALE)
                if tpl is None:
                    print(f"Failed to read template {tpl_path}; skipping")
                    continue
                templates.append({"filename": filename, "name": meta.get("name"), "glyph": meta.get("glyph"), "token": meta.get("token"), "tpl": tpl})

            print(f"Loaded {len(templates)} symbol templates from {symbols_dir}")

    summary = {
        "atlas": str(atlas_path),
        "atlas_size": [atlas_w, atlas_h],
        "card_size": [card_w, card_h],
        "cols": cols,
        "rows": rows,
        "cards": []
    }

    total = cols * rows
    print(f"Processing atlas {atlas_path} ({atlas_w}x{atlas_h}), card size {card_w}x{card_h} => {cols} cols × {rows} rows = {total} cards")

    for card_index in range(total):
        col = card_index % cols
        row = card_index // cols
        offset_x = col * card_w
        offset_y = row * card_h
        card_entry = {"card_index": card_index, "col": col, "row": row, "regions": []}

        for i, r in enumerate(regions):
            name = r.get("name") or f"region{i}"
            x = int(r["x"])
            y = int(r["y"])
            w = int(r["width"])
            h = int(r["height"])
            x_abs = offset_x + x
            y_abs = offset_y + y

            # Crop in-memory only by default
            crop = img.crop((x_abs, y_abs, x_abs + w, y_abs + h))

            text = None
            if do_ocr:
                try:
                    gray = crop.convert("L")
                    text = pytesseract.image_to_string(gray, config='--psm 6')
                except Exception as e:
                    print(f"OCR failed for card {card_index} region {i} ({name}): {e}")

            # Detect symbols (template matching) and merge glyphs into text
            matches: List[Dict[str, Any]] = []
            if templates and HAS_CV2:
                crop_gray_np = np.array(crop.convert("L"))
                crop_edges = cv2.Canny(crop_gray_np, 50, 150)
                for tpl_info in templates:
                    tpl = tpl_info["tpl"]
                    tpl_h, tpl_w = tpl.shape[:2]
                    best_color = 0.0
                    best_edge = 0.0
                    # try small set of scales to allow slight size differences
                    for scale in (0.8, 0.9, 1.0, 1.1, 1.2):
                        new_w = int(round(tpl_w * scale))
                        new_h = int(round(tpl_h * scale))
                        if new_w < 3 or new_h < 3:
                            continue
                        if new_w > crop_gray_np.shape[1] or new_h > crop_gray_np.shape[0]:
                            continue
                        try:
                            tpl_resized = cv2.resize(tpl, (new_w, new_h), interpolation=cv2.INTER_LINEAR)
                            # color/intensity match
                            res = cv2.matchTemplate(crop_gray_np, tpl_resized, cv2.TM_CCOEFF_NORMED)
                            _, max_val_color, _, _ = cv2.minMaxLoc(res)
                            # edge match verification (helps avoid JPEG artifacts)
                            tpl_edges = cv2.Canny(tpl_resized, 50, 150)
                            if tpl_edges.sum() == 0:
                                max_val_edge = 0.0
                            else:
                                res_e = cv2.matchTemplate(crop_edges, tpl_edges, cv2.TM_CCOEFF_NORMED)
                                _, max_val_edge, _, _ = cv2.minMaxLoc(res_e)
                        except Exception:
                            continue
                        if max_val_color > best_color:
                            best_color = float(max_val_color)
                        if max_val_edge > best_edge:
                            best_edge = float(max_val_edge)

                    # Accept template only if both color and edge match are strong enough
                    if best_color >= symbol_threshold and best_edge >= symbol_edge_threshold:
                        matches.append({"name": tpl_info.get("name"), "glyph": tpl_info.get("glyph"), "token": tpl_info.get("token"), "score": best_color, "edge_score": best_edge})
                        print(f"Detected symbol '{tpl_info.get('name')}' on card {card_index} region {i} (score={best_color:.3f}, edge={best_edge:.3f})")
                    else:
                        if best_color >= symbol_threshold and best_edge < symbol_edge_threshold:
                            print(f"Rejected symbol '{tpl_info.get('name')}' on card {card_index} region {i}: color={best_color:.3f} edge={best_edge:.3f} (<edge threshold {symbol_edge_threshold})")

                # Deduplicate by symbol name (keep highest score) and sort by score desc
                uniq: Dict[str, Dict[str, Any]] = {}
                for m in matches:
                    n = m.get("name") or m.get("token") or m.get("glyph") or m.get("name")
                    if n not in uniq or m.get("score", 0.0) > uniq[n].get("score", 0.0):
                        uniq[n] = m
                matches = sorted(uniq.values(), key=lambda x: x.get("score", 0.0), reverse=True)

            # Merge glyphs into OCR text (with cleanup to avoid garbage from JPEG artifacts)
            merged_text = (text or "").strip() if text is not None else ""
            if matches:
                glyphs = [m.get("glyph") or m.get("token") or m.get("name") for m in matches]
                # remove None and duplicates while keeping order
                seen = set()
                glyphs = [g for g in glyphs if g and not (g in seen or seen.add(g))]
                glyphs_str = " ".join(glyphs)

                # clean OCR text: remove control characters and isolate letters/digits/punct
                import re
                cleaned_text = re.sub(r"[\x00-\x1F\x7F]+", "", merged_text)
                # If OCR result looks like garbage (too short or mostly non-alnum), replace it with glyphs only
                alnum_count = len(re.findall(r"[A-Za-z0-9]", cleaned_text))
                if alnum_count < 2 and cleaned_text.strip() == "":
                    merged_text = glyphs_str
                elif alnum_count < 2 and len(cleaned_text) <= 3:
                    # short non-alnum/garbled OCR — prefer glyphs
                    merged_text = glyphs_str
                else:
                    merged_text = (cleaned_text + " " + glyphs_str).strip() if glyphs_str else cleaned_text


            region_entry = {
                "region_index": i,
                "name": name,
                "x": x,
                "y": y,
                "w": w,
                "h": h,
                "x_abs": x_abs,
                "y_abs": y_abs,
                "text": merged_text
            }
            card_entry["regions"].append(region_entry)

            # Optionally save individual crops and OCR text files (disabled by default)
            if save_crops:
                safe_name = f"{name.replace(' ', '_')}_card{card_index}_reg{i}"
                out_img = out_dir / f"{safe_name}.png"
                try:
                    crop.save(out_img)
                    print(f"Saved crop: {out_img} ({w}×{h} @ {x_abs},{y_abs})")
                except Exception as e:
                    print(f"Failed to save crop {out_img}: {e}")
                if merged_text:
                    txt_path = out_dir / f"{safe_name}.txt"
                    txt_path.write_text(merged_text, encoding="utf-8")

        summary["cards"].append(card_entry)

    summary_path = out_dir / "cards_summary.json"
    summary_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"Wrote summary: {summary_path}")


def main() -> None:
    p = argparse.ArgumentParser(description="Process every card in an atlas using regions JSON and optionally run OCR.")
    p.add_argument("--atlas", required=True, help="Path to atlas image (PNG/JPG)")
    p.add_argument("--regions", required=True, help="Path to regions JSON produced by the app")
    p.add_argument("--out", default="out", help="Output directory")
    p.add_argument("--ocr", action="store_true", help="Run OCR on each region if pytesseract is available")
    p.add_argument("--card-width", type=int, help="Card width in pixels (required if regions JSON has no image_size)")
    p.add_argument("--card-height", type=int, help="Card height in pixels (required if regions JSON has no image_size)")
    p.add_argument("--save-crops", action="store_true", help="If set, save cropped regions and OCR text files (disabled by default)")
    p.add_argument("--symbols-dir", help="Directory with symbol template PNGs (optional)")
    p.add_argument("--symbols-map", help="JSON mapping file for symbols (optional)")
    p.add_argument("--symbol-threshold", type=float, default=0.6, help="Template match threshold (0..1); default 0.6")
    p.add_argument("--symbol-edge-threshold", type=float, default=0.2, help="Edge-match threshold for verification (0..1); default 0.2")
    args = p.parse_args()

    atlas = Path(args.atlas)
    regions_path = Path(args.regions)
    out_dir = Path(args.out)

    if not atlas.exists():
        p.error(f"Atlas image not found: {atlas}")
    if not regions_path.exists():
        p.error(f"Regions JSON not found: {regions_path}")

    regions, image_size = load_regions(regions_path)
    symbols_dir = Path(args.symbols_dir) if args.symbols_dir else None
    symbols_map = Path(args.symbols_map) if args.symbols_map else None
    process_atlas(atlas, regions, image_size, out_dir, args.ocr, args.save_crops, args.card_width, args.card_height, symbols_dir, symbols_map, args.symbol_threshold, args.symbol_edge_threshold)


if __name__ == "__main__":
    main()
