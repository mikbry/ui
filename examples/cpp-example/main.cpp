#include <iostream>
#include <memory>

#include "../../bindings/cpp/mkui.hpp"

int main() {
    try {
        std::cout << "mkui C++ Example - Version: " << mkui::App::version() << std::endl;

        auto app = mkui::createApp();
        auto root = app->root();

        // Header
        auto header = app->viewChild(root, "border-b");
        app->textChild(header, "miklabs/ui C++ Example",
                       mkui::TextVariant::Heading2, "text-xl font-semibold");
        app->textChild(header, "C++ bindings for mkui",
                       mkui::TextVariant::Caption, "text-sm text-muted-foreground");

        // Main content
        auto content = app->viewChild(root, "flex-1");

        // Hero
        auto hero = app->viewChild(content, "text-center mb-12");
        app->textChild(hero, "mkui C++ Bindings Demo",
                       mkui::TextVariant::Heading1,
                       "text-4xl font-bold tracking-tight text-foreground mb-4");
        app->textChild(hero, "Cross-platform UI library with modern C++ API",
                       mkui::TextVariant::Caption, "text-xl text-muted-foreground");

        // Button showcase
        auto card = app->viewChild(content, "rounded-lg border bg-card p-6");
        app->textChild(card, "Button Components",
                       mkui::TextVariant::Heading2,
                       "text-2xl font-semibold leading-none tracking-tight");

        auto button_row = app->viewChild(card, "flex flex-wrap gap-4");
        auto on_primary = app->registerCallback([]() {
            std::cout << "Primary clicked" << std::endl;
        });
        app->buttonChild(button_row, "Primary", mkui::ButtonVariant::Primary, "", on_primary);
        app->buttonChild(button_row, "Secondary", mkui::ButtonVariant::Secondary);
        app->buttonChild(button_row, "Destructive", mkui::ButtonVariant::Destructive);
        app->buttonChild(button_row, "Outline", mkui::ButtonVariant::Outline);
        app->buttonChild(button_row, "Ghost", mkui::ButtonVariant::Ghost);
        app->buttonChild(button_row, "Link", mkui::ButtonVariant::Link);

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
