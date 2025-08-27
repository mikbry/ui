#include <stdio.h>
#include <stdlib.h>

// Include the generated C header
#include "mkui_c.h"

void check_result(MkuiResult result, const char* operation) {
    if (result.code != 0) {
        fprintf(stderr, "Error in %s: %s (code: %d)\n", 
                operation, 
                result.message ? result.message : "Unknown error", 
                result.code);
        if (result.message) {
            mkui_free_error_message((char*)result.message);
        }
        exit(1);
    }
}

int main() {
    printf("mkui C Example - Version: %s\n", mkui_version());
    
    // Create a new mkui application
    MkuiApp* app = mkui_app_new();
    if (!app) {
        fprintf(stderr, "Failed to create mkui application\n");
        return 1;
    }
    
    // Build the UI
    MkuiResult result;
    
    // Add main container
    result = mkui_app_add_view(app, "flex-1");
    check_result(result, "adding main view");
    
    // Add header view
    result = mkui_app_add_view(app, "border-b");
    check_result(result, "adding header view");
    
    // Add header container
    result = mkui_app_add_view(app, "container mx-auto px-4 h-16 flex items-center justify-between");
    check_result(result, "adding header container");
    
    // Add title
    result = mkui_app_add_text(app, "miklabs/ui C Example", "text-xl font-semibold");
    check_result(result, "adding title text");
    
    // Add subtitle
    result = mkui_app_add_text(app, "C bindings for mkui", "text-sm text-muted-foreground");
    check_result(result, "adding subtitle text");
    
    // Add main content area
    result = mkui_app_add_view(app, "flex-1");
    check_result(result, "adding content area");
    
    result = mkui_app_add_view(app, "container mx-auto py-8 px-4 max-w-4xl space-y-8");
    check_result(result, "adding content container");
    
    // Add hero section
    result = mkui_app_add_view(app, "text-center mb-12");
    check_result(result, "adding hero section");
    
    result = mkui_app_add_text(app, "mkui C Bindings Demo", "text-4xl font-bold tracking-tight text-foreground mb-4");
    check_result(result, "adding hero title");
    
    result = mkui_app_add_text(app, "Cross-platform UI library accessible from C", "text-xl text-muted-foreground");
    check_result(result, "adding hero description");
    
    // Add button showcase
    result = mkui_app_add_view(app, "rounded-lg border bg-card text-card-foreground shadow-sm p-6");
    check_result(result, "adding button showcase container");
    
    result = mkui_app_add_text(app, "Button Components", "text-2xl font-semibold leading-none tracking-tight");
    check_result(result, "adding button section title");
    
    result = mkui_app_add_text(app, "Various button styles and variants", "text-sm text-muted-foreground mt-2");
    check_result(result, "adding button section description");
    
    // Add buttons container
    result = mkui_app_add_view(app, "flex flex-wrap gap-4");
    check_result(result, "adding buttons container");
    
    // Add different button variants
    result = mkui_app_add_button(app, "Primary", "", MKUI_BUTTON_PRIMARY);
    check_result(result, "adding primary button");
    
    result = mkui_app_add_button(app, "Secondary", "", MKUI_BUTTON_SECONDARY);
    check_result(result, "adding secondary button");
    
    result = mkui_app_add_button(app, "Destructive", "", MKUI_BUTTON_DESTRUCTIVE);
    check_result(result, "adding destructive button");
    
    result = mkui_app_add_button(app, "Outline", "", MKUI_BUTTON_OUTLINE);
    check_result(result, "adding outline button");
    
    result = mkui_app_add_button(app, "Ghost", "", MKUI_BUTTON_GHOST);
    check_result(result, "adding ghost button");
    
    // Run the application
    printf("Starting mkui application...\n");
    result = mkui_app_run_console(app);
    if (result.code != 0) {
        fprintf(stderr, "Application run failed: %s (code: %d)\n", 
                result.message ? result.message : "Unknown error", 
                result.code);
        if (result.message) {
            mkui_free_error_message((char*)result.message);
        }
    }
    
    // Clean up
    mkui_app_free(app);
    
    printf("mkui C Example completed successfully!\n");
    return 0;
}