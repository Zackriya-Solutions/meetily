#!/usr/bin/env python3
"""
Generate Clearminutes Instagram profile icon (1080×1080 px).
Run with: ~/.local/bin/uv run --with cairosvg scripts/gen_instagram_icon.py
"""

import sys, os

try:
    import cairosvg
except ImportError:
    print("ERROR: cairosvg not found. Run with: uv run --with cairosvg scripts/gen_instagram_icon.py")
    sys.exit(1)

# Instagram: square crop, typically displayed as a circle — design fills the
# safe zone (≈80% of 1080) so nothing important is clipped by the circle mask.

SVG = """<svg width="1080" height="1080" viewBox="0 0 1080 1080" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%"   stop-color="#1e2028"/>
      <stop offset="100%" stop-color="#2c2e38"/>
    </linearGradient>
    <linearGradient id="arc" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%"   stop-color="#00d4aa"/>
      <stop offset="100%" stop-color="#0099ff"/>
    </linearGradient>
    <linearGradient id="dot" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%"   stop-color="#00d4aa"/>
      <stop offset="100%" stop-color="#0099ff"/>
    </linearGradient>
    <filter id="glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="26" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="softglow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="13" result="blur"/>
      <feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>

  <!-- Full-bleed background -->
  <rect width="1080" height="1080" fill="url(#bg)"/>

  <!-- Subtle radial depth vignette -->
  <radialGradient id="vignette" cx="50%" cy="50%" r="70%">
    <stop offset="0%"   stop-color="#ffffff" stop-opacity="0.02"/>
    <stop offset="100%" stop-color="#000000" stop-opacity="0.30"/>
  </radialGradient>
  <rect width="1080" height="1080" fill="url(#vignette)"/>

  <!--
    Fill the full 1080px canvas.
    Scale = 1080/1024 = 1.055.
    tx = 540 - 512*1.055 = 540 - 540 = 0
    ty = 540 - 512*1.055 = 540 - 540 = 0
  -->
  <g transform="translate(0, 0) scale(1.055)">

    <!-- Dashed orbit ring — same radius as arc -->
    <circle cx="512" cy="512" r="290" fill="none"
      stroke="#0099ff" stroke-width="14" stroke-dasharray="40 28" opacity="0.30"/>

    <!-- Main arc — teal → blue with glow, thicker stroke -->
    <path d="M 512 222 A 290 290 0 1 1 222 512"
      fill="none"
      stroke="url(#arc)"
      stroke-width="58"
      stroke-linecap="round"
      filter="url(#glow)"/>

    <!-- Hexagon frame — thicker -->
    <polygon points="512,312 630,374 630,498 512,560 394,498 394,374"
      fill="none" stroke="#00d4aa" stroke-width="20" stroke-opacity="0.28"/>

    <!-- 12 o'clock tick — thicker and longer -->
    <line x1="512" y1="196" x2="512" y2="270"
      stroke="#00d4aa" stroke-width="32" stroke-linecap="round"
      filter="url(#softglow)"/>

    <!-- 3 o'clock tick — thicker -->
    <line x1="820" y1="512" x2="760" y2="512"
      stroke="#0099ff" stroke-width="22" stroke-linecap="round" opacity="0.45"/>

    <!-- Centre dot — near bottom of hexagon -->
    <circle cx="512" cy="515" r="60" fill="url(#dot)" filter="url(#glow)"/>

    <!-- Arc start anchor dot — larger -->
    <circle cx="512" cy="222" r="28" fill="#00d4aa" filter="url(#softglow)"/>

  </g>


</svg>"""

out_dir = os.path.join(os.path.dirname(__file__), '..', 'frontend', 'public')
out_dir = os.path.abspath(out_dir)
os.makedirs(out_dir, exist_ok=True)

out_path = os.path.join(out_dir, 'instagram_profile.png')

print("Rendering 1080×1080 Instagram profile icon…")
cairosvg.svg2png(
    bytestring=SVG.encode(),
    write_to=out_path,
    output_width=1080,
    output_height=1080,
)
print(f"✅  Written to: {out_path}")


