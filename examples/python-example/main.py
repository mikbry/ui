#!/usr/bin/env python3
"""mkui Python Example — handle-based, runtime-backed (Sprint 4)."""

import os
import sys

venv_path = "../../crates/mkui-py/.venv/lib/python3.13/site-packages"
if os.path.exists(venv_path):
    sys.path.insert(0, venv_path)
else:
    sys.path.insert(0, "../../target/release")

try:
    import mkui_py
except ImportError:
    print("Error: mkui_py module not found.")
    print("Build it with: cd ../../crates/mkui-py && maturin develop --release")
    sys.exit(1)


def on_primary():
    print("Primary clicked")


def main():
    print(f"mkui Python Example - Version: {mkui_py.version()}")

    app = mkui_py.App()
    root = app.root()

    header = app.view_child(root, "border-b")
    app.text_child(header, "miklabs/ui Python Example",
                   mkui_py.TEXT_HEADING_2, "text-xl font-semibold")
    app.text_child(header, "Python bindings for mkui",
                   mkui_py.TEXT_CAPTION, "text-sm text-muted-foreground")

    content = app.view_child(root, "flex-1")
    hero = app.view_child(content, "text-center mb-12")
    app.text_child(hero, "mkui Python Bindings Demo",
                   mkui_py.TEXT_HEADING_1,
                   "text-4xl font-bold tracking-tight text-foreground mb-4")
    app.text_child(hero, "Cross-platform UI library with Pythonic API",
                   mkui_py.TEXT_CAPTION, "text-xl text-muted-foreground")

    button_row = app.view_child(content, "flex flex-wrap gap-4")
    on_click = app.register_callback(on_primary)
    app.button_child(button_row, "Primary", mkui_py.BUTTON_PRIMARY, "", on_click)
    app.button_child(button_row, "Secondary", mkui_py.BUTTON_SECONDARY, "")
    app.button_child(button_row, "Destructive", mkui_py.BUTTON_DESTRUCTIVE, "")
    app.button_child(button_row, "Outline", mkui_py.BUTTON_OUTLINE, "")
    app.button_child(button_row, "Ghost", mkui_py.BUTTON_GHOST, "")
    app.button_child(button_row, "Link", mkui_py.BUTTON_LINK, "")

    print(f"Tree size: {app.node_count()} nodes")
    print("Starting mkui application...")
    app.run_console()


if __name__ == "__main__":
    sys.exit(main() or 0)
