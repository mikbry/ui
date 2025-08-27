use mkui_core::headless::{ToggleBuilder};

fn main() {
    // Example of using the headless toggle component
    let mut toggle = ToggleBuilder::new()
        .checked(false)
        .on_change(|checked| {
            println!("Toggle changed to: {}", checked);
        })
        .build();
    
    println!("Initial state: {}", toggle.is_checked());
    
    // Simulate user interaction
    toggle.toggle();
    println!("After toggle: {}", toggle.is_checked());
    
    // Simulate keyboard interaction
    use mkui_core::headless::KeyboardInteractable;
    toggle.handle_key_down(" ");
    println!("After space key: {}", toggle.is_checked());
}