/* WARNING: This handwritten header tracks the public mkui-c API.
 * cbindgen also emits a generated copy at <target>/include/mkui_c.h —
 * keep this file in sync when adding/removing FFI symbols.
 */

#ifndef MKUI_C_H
#define MKUI_C_H

#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

/* Opaque handle to a Mkui application instance */
typedef struct MkuiApp MkuiApp;

/* Opaque node id — (index, generation) pair guards against use-after-free. */
typedef struct MkuiNodeId {
    uint32_t index;
    uint32_t generation;
} MkuiNodeId;

/* Opaque action id (same shape as MkuiNodeId; different allocation arena). */
typedef struct MkuiActionId {
    uint32_t index;
    uint32_t generation;
} MkuiActionId;

/* Error codes for mkui operations */
typedef enum MkuiErrorCode {
    MKUI_SUCCESS = 0,
    MKUI_INITIALIZATION_FAILED = 1,
    MKUI_INVALID_PARAMETER = 2,
    MKUI_RUNTIME_ERROR = 3,
    MKUI_OUT_OF_MEMORY = 4,
} MkuiErrorCode;

/* Result type for C API functions */
typedef struct MkuiResult {
    MkuiErrorCode code;
    const char* message;
} MkuiResult;

/* Button variant constants */
extern const int MKUI_BUTTON_PRIMARY;
extern const int MKUI_BUTTON_SECONDARY;
extern const int MKUI_BUTTON_DESTRUCTIVE;
extern const int MKUI_BUTTON_OUTLINE;
extern const int MKUI_BUTTON_GHOST;
extern const int MKUI_BUTTON_LINK;

/* Text variant constants */
extern const int MKUI_TEXT_BODY;
extern const int MKUI_TEXT_HEADING_1;
extern const int MKUI_TEXT_HEADING_2;
extern const int MKUI_TEXT_HEADING_3;
extern const int MKUI_TEXT_CAPTION;
extern const int MKUI_TEXT_LABEL;
extern const int MKUI_TEXT_CODE;

/* Application lifecycle */
MkuiApp* mkui_app_new(void);
void mkui_app_free(MkuiApp* app);

/* Tree construction — handle-based, nested. The root is a synthetic
 * top-level container; attach children to `mkui_app_root(app)` to put them
 * directly under the root.
 */
MkuiNodeId mkui_app_root(const MkuiApp* app);
MkuiNodeId mkui_app_view_child(MkuiApp* app, MkuiNodeId parent, const char* class_name);
MkuiNodeId mkui_app_text_child(MkuiApp* app,
                                MkuiNodeId parent,
                                const char* content,
                                int variant,
                                const char* class_name);
MkuiNodeId mkui_app_button_child(MkuiApp* app,
                                  MkuiNodeId parent,
                                  const char* label,
                                  int variant,
                                  const char* class_name,
                                  MkuiActionId on_press);

/* Action callbacks. `mkui_app_register_callback` returns an action id the
 * host passes to `mkui_app_button_child`; firing the action runs `func`
 * with `user_data`. mkui never frees `user_data`. Pass
 * `(MkuiActionId){UINT32_MAX, UINT32_MAX}` to mean "no callback".
 */
MkuiActionId mkui_app_register_callback(MkuiApp* app,
                                         void (*func)(void* user_data),
                                         void* user_data);
MkuiResult mkui_app_fire_action(MkuiApp* app, MkuiActionId id);

/* Application execution */
MkuiResult mkui_app_run_console(MkuiApp* app);

/* Parity-test snapshot — returns a heap-allocated NUL-terminated JSON
 * string. The host frees it with `mkui_free_error_message`. */
char* mkui_app_snapshot_json(const MkuiApp* app);

/* Memory management */
void mkui_free_error_message(char* message);

/* Utility */
const char* mkui_version(void);

#ifdef __cplusplus
}
#endif

#endif /* MKUI_C_H */
