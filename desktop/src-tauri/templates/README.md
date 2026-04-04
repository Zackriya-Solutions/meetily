# Summary Templates

This directory contains bundled summary templates for the native desktop app.

## Loader Behavior

The template loader currently resolves templates in this order:

1. User custom templates in the app data directory
2. Bundled template files
3. Built-in embedded templates

## Current Template Commands

The native app exposes commands to:

- list templates
- fetch template details
- validate template JSON

## Custom Template Location

Custom templates are loaded from the app data directory under `MeetFree/templates`.

Examples used by the loader:

- macOS: `~/Library/Application Support/MeetFree/templates/`
- Windows: `%APPDATA%\\MeetFree\\templates\\`
- Linux: `~/.config/MeetFree/templates/`

## Template Shape

Each template JSON file includes:

- `name`
- `description`
- `sections`

Each section includes:

- `title`
- `instruction`
- `format`

Optional section fields such as `item_format` are also supported by the current parser.
