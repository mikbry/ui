#!/usr/bin/env python3
"""
mkui Python Example

This example demonstrates how to use mkui from Python to create 
a cross-platform console application.
"""

import sys
import os

# Add the built mkui-py module to the path
# Check for virtual environment installation first
venv_path = "../../crates/mkui-py/.venv/lib/python3.13/site-packages"
if os.path.exists(venv_path):
    sys.path.insert(0, venv_path)
else:
    # Fallback to target/release (for cargo build)
    sys.path.insert(0, "../../target/release")

try:
    import mkui_py
except ImportError:
    print("Error: mkui_py module not found.")
    print("Please build the Python bindings first:")
    print("  cd ../../crates/mkui-py")
    print("  uv venv")
    print("  source .venv/bin/activate")
    print("  maturin develop --release")
    sys.exit(1)

def main():
    """Main function demonstrating mkui Python API"""
    print(f"mkui Python Example - Version: {mkui_py.version()}")
    
    try:
        # Create application using the high-level API
        app = mkui_py.create_app()
        
        # Build UI step by step (PyO3 limitation - no true method chaining)
        # Main container
        app.view("flex-1")
        
        # Header
        app.view("border-b")
        app.view("container mx-auto px-4 h-16 flex items-center justify-between")
        app.text("miklabs/ui Python Example", "text-xl font-semibold")
        app.text("Python bindings for mkui", "text-sm text-muted-foreground")
        
        # Main content
        app.view("flex-1")
        app.view("container mx-auto py-8 px-4 max-w-4xl space-y-8")
        
        # Hero section
        app.view("text-center mb-12")
        app.text("mkui Python Bindings Demo", "text-4xl font-bold tracking-tight text-foreground mb-4")
        app.text("Cross-platform UI library with Pythonic API", "text-xl text-muted-foreground")
        
        # Button showcase
        app.view("rounded-lg border bg-card text-card-foreground shadow-sm p-6")
        app.text("Button Components", "text-2xl font-semibold leading-none tracking-tight")
        app.text("Various button styles and variants", "text-sm text-muted-foreground mt-2")
        
        # Buttons container
        app.view("flex flex-wrap gap-4")
        app.button("Primary", mkui_py.BUTTON_PRIMARY)
        app.button("Secondary", mkui_py.BUTTON_SECONDARY) 
        app.button("Destructive", mkui_py.BUTTON_DESTRUCTIVE)
        app.button("Outline", mkui_py.BUTTON_OUTLINE)
        app.button("Ghost", mkui_py.BUTTON_GHOST)
        app.button("Link", mkui_py.BUTTON_LINK)
        
        # Run the application
        print("Starting mkui application...")
        app.run_console()
        
        print("mkui Python Example completed successfully!")
        
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
        
    return 0

def showcase_example():
    """Example using the convenience showcase function"""
    print("Running mkui showcase from Python...")
    try:
        mkui_py.run_showcase()
    except Exception as e:
        print(f"Showcase error: {e}", file=sys.stderr)

def step_by_step_example():
    """Example showing alternative step-by-step API"""
    print("Step-by-step API example...")
    
    try:
        app = mkui_py.App()
        
        # Add components one by one using add_* methods
        app.add_view("container mx-auto p-4")
        app.add_text("Step-by-step Example", "text-2xl font-bold mb-4")
        app.add_text("This shows the add_* methods", "text-gray-600 mb-4")
        app.add_button("Click me!", mkui_py.BUTTON_PRIMARY, "px-4 py-2")
        
        app.run_console()
        
    except Exception as e:
        print(f"Step-by-step example error: {e}", file=sys.stderr)

if __name__ == "__main__":
    print("Choose an example to run:")
    print("1. Full UI example (step-by-step API)")
    print("2. Showcase example")
    print("3. Alternative step-by-step example (add_* methods)")
    
    choice = input("Enter choice (1-3, default 1): ").strip() or "1"
    
    if choice == "1":
        sys.exit(main())
    elif choice == "2":
        showcase_example()
    elif choice == "3":
        step_by_step_example()
    else:
        print("Invalid choice")
        sys.exit(1)