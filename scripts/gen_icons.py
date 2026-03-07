#!/usr/bin/env python3
# /// script
# requires-python = "==3.13.*"
# dependencies = ["cairosvg", "Pillow"]
# ///
"""
Generate all Clearminutes app icons from SVG source.
Run with:
  DYLD_LIBRARY_PATH=/opt/homebrew/lib uv run --python 3.13 scripts/gen_icons.py
"""

import sys
print("Starting icon generation...", flush=True)

try:
    import cairosvg
    print("cairosvg imported", flush=True)
except ImportError as e:
    print(f"ERROR importing cairosvg: {e}", flush=True)
    sys.exit(1)

try:
    from PIL import Image, ImageDraw
    print("Pillow imported", flush=True)
except ImportError as e:
    print(f"ERROR importing Pillow: {e}", flush=True)
    sys.exit(1)

import io
import os
import struct
import zlib

# ── SVG source — Dark theme (charcoal bg, teal/blue arc) ──────────────────────
# This is the definitive 1024×1024 master used to generate all sizes.
SVG = """<svg width="1024" height="1024" viewBox="0 0 1024 1024" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#25272f"/>
      <stop offset="100%" stop-color="#2c2e38"/>
    </linearGradient>
    <linearGradient id="arc" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#00d4aa"/>
      <stop offset="100%" stop-color="#0099ff"/>
    </linearGradient>
    <linearGradient id="center" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#00d4aa"/>
      <stop offset="100%" stop-color="#0099ff"/>
    </linearGradient>
    <filter id="glow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="14" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="softglow" x="-10%" y="-10%" width="120%" height="120%">
      <feGaussianBlur stdDeviation="6" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <!-- Top-left specular bevel — mimics macOS icon lighting -->
    <radialGradient id="bevel" cx="28%" cy="22%" r="55%" fx="20%" fy="14%">
      <stop offset="0%"   stop-color="#ffffff" stop-opacity="0.13"/>
      <stop offset="45%"  stop-color="#ffffff" stop-opacity="0.04"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.00"/>
    </radialGradient>
    <!-- Inner rim: bright top-left edge, fades to nothing bottom-right -->
    <linearGradient id="rim" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%"   stop-color="#ffffff" stop-opacity="0.10"/>
      <stop offset="50%"  stop-color="#ffffff" stop-opacity="0.02"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.00"/>
    </linearGradient>
  </defs>

  <!--
    Apple HIG: artwork should occupy ~80% of the canvas so macOS can apply
    its own squircle mask and the icon appears the same visual size as peers.
    Canvas: 1024×1024.  Content area: 820×820 centred → inset ~102px each side.
    We achieve this by wrapping everything in a <g> that scales 0.80 and
    translates to centre — equivalent to the artwork living in a 820px box.

    The background squircle is included here so the icon has its own dark
    background on transparent surfaces (e.g. Finder list view, Spotlight).
    macOS will clip it to its own mask shape automatically.
  -->
  <g transform="translate(102, 102) scale(0.8)">

    <!-- Background squircle -->
    <rect width="1024" height="1024" rx="230" fill="url(#bg)"/>

    <!-- Dashed orbit ring -->
    <circle cx="512" cy="512" r="290" fill="none"
      stroke="#0099ff" stroke-width="6" stroke-dasharray="28 20" opacity="0.30"/>

    <!-- Main arc — glowing teal-to-blue -->
    <path d="M 512 222 A 290 290 0 1 1 222 512"
      fill="none"
      stroke="url(#arc)"
      stroke-width="28"
      stroke-linecap="round"
      filter="url(#glow)"/>

    <!-- Hexagon frame -->
    <polygon points="512,312 630,374 630,498 512,560 394,498 394,374"
      fill="none"
      stroke="#00d4aa"
      stroke-width="10"
      stroke-opacity="0.30"/>

    <!-- 12-o-clock tick mark -->
    <line x1="512" y1="196" x2="512" y2="248"
      stroke="#00d4aa" stroke-width="16" stroke-linecap="round"
      filter="url(#softglow)"/>

    <!-- 3-o-clock subtle tick -->
    <line x1="818" y1="512" x2="776" y2="512"
      stroke="#0099ff" stroke-width="10" stroke-linecap="round" opacity="0.5"/>

    <!-- Centre dot -->
    <circle cx="512" cy="512" r="36"
      fill="url(#center)"
      filter="url(#glow)"/>

    <!-- Top arc anchor dot -->
    <circle cx="512" cy="222" r="16" fill="#00d4aa" filter="url(#softglow)"/>

    <!-- Bevel: top-left specular highlight (painted last, on top of all content) -->
    <rect width="1024" height="1024" rx="230" fill="url(#bevel)"/>
    <!-- Rim: subtle inner edge highlight strongest at top-left -->
    <rect width="1024" height="1024" rx="230" fill="none" stroke="url(#rim)" stroke-width="6"/>

  </g>
</svg>"""

ICONS_DIR = os.path.join(os.path.dirname(__file__), '..', 'frontend', 'src-tauri', 'icons')
ICONS_DIR = os.path.abspath(ICONS_DIR)
print(f"Output dir: {ICONS_DIR}", flush=True)

def svg_to_png(svg_str: str, size: int) -> bytes:
    return cairosvg.svg2png(bytestring=svg_str.encode(), output_width=size, output_height=size)

def save_png(data: bytes, path: str):
    os.makedirs(os.path.dirname(path) if os.path.dirname(path) else '.', exist_ok=True)
    with open(path, 'wb') as f:
        f.write(data)
    print(f"  wrote {os.path.basename(path)} ({len(data)//1024} KB)", flush=True)

# ── 1. Generate all PNG sizes ─────────────────────────────────────────────────
print("\n[1/4] Generating PNGs...", flush=True)

sizes = {
    'icon.png':             1024,
    'icon_16x16.png':       16,
    'icon_16x16@2x.png':    32,
    'icon_32x32.png':       32,
    'icon_32x32@2x.png':    64,
    'icon_128x128.png':     128,
    'icon_128x128@2x.png':  256,
    'icon_256x256.png':     256,
    'icon_256x256@2x.png':  512,
    'icon_512x512.png':     512,
    'icon_512x512@2x.png':  1024,
    '128x128@2x.png':       256,
    '32x32.png':            32,
}

png_cache: dict[int, bytes] = {}

for filename, px in sizes.items():
    if px not in png_cache:
        png_cache[px] = svg_to_png(SVG, px)
    save_png(png_cache[px], os.path.join(ICONS_DIR, filename))

# Also generate Windows Square logos
win_sizes = {
    'Square30x30Logo.png':   30,
    'Square44x44Logo.png':   44,
    'Square71x71Logo.png':   71,
    'Square89x89Logo.png':   89,
    'Square107x107Logo.png': 107,
    'Square142x142Logo.png': 142,
    'Square150x150Logo.png': 150,
    'Square284x284Logo.png': 284,
    'Square310x310Logo.png': 310,
    'StoreLogo.png':         50,
}
for filename, px in win_sizes.items():
    if px not in png_cache:
        png_cache[px] = svg_to_png(SVG, px)
    save_png(png_cache[px], os.path.join(ICONS_DIR, filename))

# ── 2. Generate .icns (macOS) ─────────────────────────────────────────────────
print("\n[2/4] Building .icns...", flush=True)

# macOS iconset sizes (name → pixel size, is_retina)
ICNS_ENTRIES = [
    ('icp4', 16),   ('icp5', 32),   ('icp6', 64),
    ('ic07', 128),  ('ic08', 256),  ('ic09', 512),  ('ic10', 1024),
    ('ic11', 32),   ('ic12', 64),   ('ic13', 256),  ('ic14', 512),
]

def make_icns(png_cache: dict) -> bytes:
    """Build a minimal ICNS binary from PNG data."""
    chunks = []
    for ostype, px in ICNS_ENTRIES:
        data = png_cache.get(px) or svg_to_png(SVG, px)
        png_cache[px] = data
        chunk_len = 8 + len(data)
        chunks.append(ostype.encode('ascii') + struct.pack('>I', chunk_len) + data)
    body = b''.join(chunks)
    header = b'icns' + struct.pack('>I', 8 + len(body))
    return header + body

icns_data = make_icns(png_cache)

for fname in ('icon.icns', 'app_icon.icns'):
    path = os.path.join(ICONS_DIR, fname)
    with open(path, 'wb') as f:
        f.write(icns_data)
    print(f"  wrote {fname} ({len(icns_data)//1024} KB)", flush=True)

# ── 3. Generate .ico (Windows) ────────────────────────────────────────────────
print("\n[3/4] Building .ico...", flush=True)

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

def make_ico(png_cache: dict) -> bytes:
    """Build a multi-size ICO file."""
    images = []
    for px in ICO_SIZES:
        data = png_cache.get(px) or svg_to_png(SVG, px)
        png_cache[px] = data
        # For ICO, sizes >256 are clamped; 256 stored as 0
        w = h = 0 if px >= 256 else px
        images.append((w, h, data))

    # ICO header: 6 bytes
    header = struct.pack('<HHH', 0, 1, len(images))
    # Each directory entry: 16 bytes
    offset = 6 + 16 * len(images)
    directory = b''
    data_blob = b''
    for w, h, data in images:
        directory += struct.pack('<BBBBHHII', w, h, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        data_blob += data
    return header + directory + data_blob

ico_data = make_ico(png_cache)

for fname in ('icon.ico', 'app_icon.ico'):
    path = os.path.join(ICONS_DIR, fname)
    with open(path, 'wb') as f:
        f.write(ico_data)
    print(f"  wrote {fname} ({len(ico_data)//1024} KB)", flush=True)

# ── 4. Copy master to public folder for About dialog ─────────────────────────
print("\n[4/4] Copying to public/...", flush=True)
public_dir = os.path.join(os.path.dirname(__file__), '..', 'frontend', 'public')
public_dir = os.path.abspath(public_dir)
master = png_cache[1024]
for name in ('icon_128x128.png',):
    dst = os.path.join(public_dir, name)
    save_png(png_cache[128], dst)

# ── 5. Generate macOS menu-bar template icons ─────────────────────────────────
# Template icons must be black on a transparent background.
# macOS automatically inverts them for dark menu bars / dark mode.
# Standard sizes: 18pt (1x = 18px, 2x = 36px).
print("\n[5/5] Generating macOS tray template icons...", flush=True)

def make_tray_icon_png(px: int) -> bytes:
    """Draw a minimal arc+dot menu-bar icon using Pillow (no cairo needed)."""
    img = Image.new("RGBA", (px, px), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    cx, cy = px / 2, px / 2
    r = px * 0.38
    lw = max(1, round(px * 0.11))
    margin = round(cx - r)
    bbox = [margin, margin, px - margin, px - margin]
    # Arc: top (-90°) clockwise 270° to left side (180°)
    draw.arc(bbox, start=-90, end=180, fill=(0, 0, 0, 255), width=lw)
    # 12-o-clock tick
    draw.line([(cx, cy - r - lw), (cx, cy - r + lw * 1.5)],
              fill=(0, 0, 0, 255), width=lw)
    # Centre dot
    dot_r = max(1, round(px * 0.09))
    draw.ellipse([cx - dot_r, cy - dot_r, cx + dot_r, cy + dot_r],
                 fill=(0, 0, 0, 255))
    buf = io.BytesIO()
    img.save(buf, "PNG")
    return buf.getvalue()

for fname, px in [("tray-icon.png", 18), ("tray-icon@2x.png", 36)]:
    data = make_tray_icon_png(px)
    save_png(data, os.path.join(ICONS_DIR, fname))

print("\n✅ All icons generated successfully!", flush=True)

