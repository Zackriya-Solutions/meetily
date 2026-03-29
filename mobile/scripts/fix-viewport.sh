#!/bin/bash
# Remove the Next.js auto-generated viewport tag (the one WITHOUT viewport-fit)
# Keeps our tag which has viewport-fit=cover
find out -name "*.html" -exec sed -i '' \
  's|<meta name="viewport" content="width=device-width, initial-scale=1"/>||g' \
  {} +
echo "Fixed viewport in $(find out -name '*.html' | wc -l | tr -d ' ') files"
