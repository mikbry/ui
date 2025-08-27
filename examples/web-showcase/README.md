# mkui Web Showcase

Interactive demo of mkui components running in the browser via WebAssembly.

## Prerequisites

- Rust toolchain
- wasm-pack (will be installed automatically by build script)

## Build

```bash
./build.sh
```

## Run

After building, serve the files locally:

```bash
# Using Python
python3 -m http.server 8000

# Or using Node.js
npx serve .

# Or any other static file server
```

Then open http://localhost:8000 in your browser.

## Development

The showcase demonstrates:
- Headless component architecture
- Web rendering via DOM manipulation
- State management
- Event handling
- Tailwind-like styling system