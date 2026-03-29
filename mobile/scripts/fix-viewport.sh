#!/bin/bash
# Fix Next.js viewport override — patches BOTH static HTML and RSC hydration payload
find out -name "*.html" -exec sed -i '' \
  -e 's|<meta name="viewport" content="width=device-width, initial-scale=1"/>||g' \
  -e 's|"content":"width=device-width, initial-scale=1"|"content":"width=device-width, initial-scale=1, viewport-fit=cover, user-scalable=no"|g' \
  -e 's|\\\"content\\\":\\\"width=device-width, initial-scale=1\\\"|\\\"content\\\":\\\"width=device-width, initial-scale=1, viewport-fit=cover, user-scalable=no\\\"|g' \
  {} +
echo "Fixed viewport in $(find out -name '*.html' | wc -l | tr -d ' ') files"
