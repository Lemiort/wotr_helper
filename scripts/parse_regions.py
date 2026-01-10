#!/usr/bin/env python3
"""
parse_regions.py

Usage:
    python scripts/parse_regions.py --atlas assets/light_cards.png --regions assets/card2.json --out out --ocr

What it does:
- Loads a regions JSON saved by the app (supports both the new object with `image_size` + `regions` and the old plain `Vec<Region>`).
- Crops each named region from the atlas image and saves it as PNG in the output directory.
- If `--ocr` is passed and pytesseract is installed (and Tesseract binary available), performs OCR on each crop and writes a corresponding .txt file.

Dependencies:
- pillow (PIL)
- pytesseract (optional; requires Tesseract OCR installed on the system)

Install deps:
    pip install pillow pytesseract

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


def process_atlas(atlas_path: Path, regions: List[Dict[str, Any]], image_size: Optional[Tuple[int, int]], out_dir: Path, do_ocr: bool, save_crops: bool, card_w_arg: Optional[int], card_h_arg: Optional[int]) -> None:
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

            region_entry = {
                "region_index": i,
                "name": name,
                "x": x,
                "y": y,
                "w": w,
                "h": h,
                "x_abs": x_abs,
                "y_abs": y_abs,
                "text": text
            }
            card_entry["regions"].append(region_entry)

            # Optionally save individual crops and OCR text (disabled by default)
            if save_crops:
                safe_name = f"{name.replace(' ', '_')}_card{card_index}_reg{i}"
                out_img = out_dir / f"{safe_name}.png"
                try:
                    crop.save(out_img)
                    print(f"Saved crop: {out_img} ({w}×{h} @ {x_abs},{y_abs})")
                except Exception as e:
                    print(f"Failed to save crop {out_img}: {e}")
                if do_ocr and text is not None:
                    txt_path = out_dir / f"{safe_name}.txt"
                    txt_path.write_text(text, encoding="utf-8")

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
    args = p.parse_args()

    atlas = Path(args.atlas)
    regions_path = Path(args.regions)
    out_dir = Path(args.out)

    if not atlas.exists():
        p.error(f"Atlas image not found: {atlas}")
    if not regions_path.exists():
        p.error(f"Regions JSON not found: {regions_path}")

    regions, image_size = load_regions(regions_path)
    process_atlas(atlas, regions, image_size, out_dir, args.ocr, args.save_crops, args.card_width, args.card_height)


if __name__ == "__main__":
    main()
