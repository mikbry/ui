/* Warning: This file provides the C API for mkui. */

#ifndef MKUI_C_H
#define MKUI_C_H

#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

/* Opaque handle to a Mkui application instance */
typedef struct MkuiApp MkuiApp;

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

/* Application lifecycle functions */
MkuiApp* mkui_app_new(void);
void mkui_app_free(MkuiApp* app);

/* Component creation functions */
MkuiResult mkui_app_add_view(MkuiApp* app, const char* class_name);
MkuiResult mkui_app_add_text(MkuiApp* app, const char* content, const char* class_name);
MkuiResult mkui_app_add_button(MkuiApp* app, const char* text, const char* class_name, int variant);

/* Application execution */
MkuiResult mkui_app_run_console(MkuiApp* app);

/* Memory management */
void mkui_free_error_message(char* message);

/* Utility functions */
const char* mkui_version(void);

#ifdef __cplusplus
}
#endif

#endif /* MKUI_C_H */