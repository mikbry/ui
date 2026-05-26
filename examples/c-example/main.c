#include <stdio.h>
#include <stdlib.h>

#include "mkui_c.h"

static const MkuiActionId NO_CALLBACK = { UINT32_MAX, UINT32_MAX };

static void on_primary_click(void* user_data) {
    (void)user_data;
    printf("Primary clicked\n");
}

static void check_result(MkuiResult result, const char* operation) {
    if (result.code != MKUI_SUCCESS) {
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

int main(void) {
    printf("mkui C Example - Version: %s\n", mkui_version());

    MkuiApp* app = mkui_app_new();
    if (!app) {
        fprintf(stderr, "Failed to create mkui application\n");
        return 1;
    }

    MkuiNodeId root = mkui_app_root(app);

    /* Header */
    MkuiNodeId header = mkui_app_view_child(app, root, "border-b");
    mkui_app_text_child(app, header, "miklabs/ui C Example",
                        MKUI_TEXT_HEADING_2,
                        "text-xl font-semibold");
    mkui_app_text_child(app, header, "C bindings for mkui",
                        MKUI_TEXT_CAPTION,
                        "text-sm text-muted-foreground");

    /* Main content */
    MkuiNodeId content = mkui_app_view_child(app, root, "flex-1");
    MkuiNodeId hero = mkui_app_view_child(app, content, "text-center mb-12");
    mkui_app_text_child(app, hero, "mkui C Bindings Demo",
                        MKUI_TEXT_HEADING_1,
                        "text-4xl font-bold tracking-tight text-foreground mb-4");
    mkui_app_text_child(app, hero, "Cross-platform UI library accessible from C",
                        MKUI_TEXT_CAPTION,
                        "text-xl text-muted-foreground");

    /* Button showcase */
    MkuiActionId on_click = mkui_app_register_callback(app, on_primary_click, NULL);
    MkuiNodeId button_row = mkui_app_view_child(app, content, "flex flex-wrap gap-4");
    mkui_app_button_child(app, button_row, "Primary", MKUI_BUTTON_PRIMARY, "", on_click);
    mkui_app_button_child(app, button_row, "Secondary", MKUI_BUTTON_SECONDARY, "", NO_CALLBACK);
    mkui_app_button_child(app, button_row, "Destructive", MKUI_BUTTON_DESTRUCTIVE, "", NO_CALLBACK);
    mkui_app_button_child(app, button_row, "Outline", MKUI_BUTTON_OUTLINE, "", NO_CALLBACK);
    mkui_app_button_child(app, button_row, "Ghost", MKUI_BUTTON_GHOST, "", NO_CALLBACK);

    /* Run */
    printf("Starting mkui application...\n");
    MkuiResult result = mkui_app_run_console(app);
    if (result.code != MKUI_SUCCESS) {
        fprintf(stderr, "Application run failed: %s (code: %d)\n",
                result.message ? result.message : "Unknown error", result.code);
        if (result.message) {
            mkui_free_error_message((char*)result.message);
        }
    }

    mkui_app_free(app);
    printf("mkui C Example completed successfully!\n");
    return 0;
}

/* Silence unused-helper warning */
static void __mkui_unused_check_result_anchor(void) { (void)check_result; }
