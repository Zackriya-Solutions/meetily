#!/bin/bash
# Fix Next.js viewport override
# 1. Remove the static duplicate viewport meta tag
# 2. Fix the RSC payload that React hydrates (replaces viewport at runtime)
find out -name "*.html" -exec sed -i '' \
  -e 's|<meta name="viewport" content="width=device-width, initial-scale=1"/>||g' \
  -e 's|"name":"viewport","content":"width=device-width, initial-scale=1"|"name":"viewport","content":"width=device-width, initial-scale=1, viewport-fit=cover, user-scalable=no"|g' \
  {} +
echo "Fixed viewport in $(find out -name '*.html' | wc -l | tr -d ' ') files"
