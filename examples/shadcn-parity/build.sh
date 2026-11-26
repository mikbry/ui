#!/bin/bash
# Builds both panes of the #278 shadcn/ui parity fixture: the mkui-web wasm
# module (mkui.html's pkg/) and the Tailwind-compiled shadcn reference pane
# CSS (shadcn.html's dist/shadcn.css). See README.md for the full workflow.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

echo "Building the mkui-web pane (wasm)..."
wasm-pack build "$here" --target web --dev --no-opt --out-dir pkg

echo "Compiling the shadcn/ui reference pane CSS..."
modules="${TAILWIND_NODE_MODULES:-$root/examples/web-showcase/node_modules}"
cli="$modules/@tailwindcss/cli/dist/index.mjs"
if [ ! -f "$cli" ]; then
  echo "Install examples/web-showcase dependencies with 'npm ci --prefix examples/web-showcase', or set TAILWIND_NODE_MODULES." >&2
  exit 1
fi
# `@import "tailwindcss"` resolves against the input file's directory, so
# this example's own node_modules is a symlink to web-showcase's install
# (same trick as examples/web-recipe-smoke/check-cascade.mjs's temp-dir
# symlink) rather than a second npm install. Never committed — see
# .gitignore.
ln -sfn "$modules" "$here/node_modules"
mkdir -p "$here/dist"
node "$cli" -i "$here/shadcn-source.css" -o "$here/dist/shadcn.css" --minify

echo ""
echo "Done. Serve with:"
echo "  python3 -m http.server 8000 --directory examples/shadcn-parity"
echo "Then open http://localhost:8000/index.html"
