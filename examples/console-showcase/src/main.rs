use showcase_common::create_showcase_ui;

fn main() -> std::io::Result<()> {
    println!("mkui: Creating console app...");
    mkui::run!(create_showcase_ui, console)
}
