/* Compile and run against the generated header and built library; see bindings/README.md. */
#include "mkui_c.h"
#include <assert.h>
#include <string.h>

static void increment(void *data) { ++*(int *)data; }

int main(void) {
    MkuiApp *app = mkui_app_new();
    assert(app);
    MkuiNodeId root = mkui_app_root(app);
    int fired = 0;
    MkuiActionId action = mkui_app_register_callback(app, increment, &fired);
    /* Six arguments: old source and binaries keep enabled semantics. */
    MkuiNodeId legacy = mkui_app_button_child(app, root, "legacy", MKUI_BUTTON_PRIMARY, "", action);
    MkuiNodeId enabled = mkui_app_button_child_with_disabled(app, root, "enabled", MKUI_BUTTON_PRIMARY, "", action, 0);
    MkuiNodeId disabled = mkui_app_button_child_with_disabled(app, root, "disabled", MKUI_BUTTON_PRIMARY, "", action, 1);
    MkuiNodeId nonzero = mkui_app_button_child_with_disabled(app, root, "nonzero", MKUI_BUTTON_PRIMARY, "", action, -1);
    assert(legacy.index != UINT32_MAX && enabled.index != UINT32_MAX);
    assert(disabled.index != UINT32_MAX && nonzero.index != UINT32_MAX);
    char *json = mkui_app_snapshot_json(app);
    assert(json);
    /* The snapshot is deterministic; count both enabled and disabled nodes. */
    const char *cursor = json;
    int active_count = 0, disabled_count = 0;
    while ((cursor = strstr(cursor, "\"disabled\":false"))) { ++active_count; ++cursor; }
    cursor = json;
    while ((cursor = strstr(cursor, "\"disabled\":true"))) { ++disabled_count; ++cursor; }
    assert(active_count == 2 && disabled_count == 2);
    mkui_free_error_message(json);
    /* Disabled controls suppress UI activation, not explicit action calls. */
    MkuiResult result = mkui_app_fire_action(app, action);
    assert(result.code == MKUI_SUCCESS && fired == 1);
    mkui_app_free(app);
    return 0;
}
