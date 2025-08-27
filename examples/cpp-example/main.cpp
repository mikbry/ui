#include <iostream>
#include <memory>

// Include the C++ wrapper
#include "../../bindings/cpp/mkui.hpp"

int main() {
    try {
        std::cout << "mkui C++ Example - Version: " << mkui::App::version() << std::endl;
        
        // Create application using RAII wrapper
        auto app = mkui::createApp();
        
        // Build the UI using method chaining
        app->addView("flex-1")
           // Header
           .addView("border-b")
           .addView("container mx-auto px-4 h-16 flex items-center justify-between")
           .addText("miklabs/ui C++ Example", "text-xl font-semibold")
           .addText("C++ bindings for mkui", "text-sm text-muted-foreground")
           
           // Main content
           .addView("flex-1")
           .addView("container mx-auto py-8 px-4 max-w-4xl space-y-8")
           
           // Hero section
           .addView("text-center mb-12")
           .addText("mkui C++ Bindings Demo", "text-4xl font-bold tracking-tight text-foreground mb-4")
           .addText("Cross-platform UI library with modern C++ API", "text-xl text-muted-foreground")
           
           // Button showcase
           .addView("rounded-lg border bg-card text-card-foreground shadow-sm p-6")
           .addText("Button Components", "text-2xl font-semibold leading-none tracking-tight")
           .addText("Various button styles and variants", "text-sm text-muted-foreground mt-2")
           
           // Buttons with different variants
           .addView("flex flex-wrap gap-4")
           .addButton("Primary", mkui::ButtonVariant::Primary)
           .addButton("Secondary", mkui::ButtonVariant::Secondary)
           .addButton("Destructive", mkui::ButtonVariant::Destructive)
           .addButton("Outline", mkui::ButtonVariant::Outline)
           .addButton("Ghost", mkui::ButtonVariant::Ghost)
           .addButton("Link", mkui::ButtonVariant::Link);
        
        // Run the application
        std::cout << "Starting mkui application..." << std::endl;
        app->runConsole();
        
        std::cout << "mkui C++ Example completed successfully!" << std::endl;
        
    } catch (const mkui::MkuiException& e) {
        std::cerr << "mkui Error: " << e.what() << " (code: " << e.code() << ")" << std::endl;
        return 1;
    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    
    return 0;
}