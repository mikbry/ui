#pragma once

#include <memory>
#include <string>
#include <stdexcept>

// Forward declare the C types
extern "C" {
    struct MkuiApp;
    
    enum MkuiErrorCode {
        MKUI_SUCCESS = 0,
        MKUI_INITIALIZATION_FAILED = 1,
        MKUI_INVALID_PARAMETER = 2,
        MKUI_RUNTIME_ERROR = 3,
        MKUI_OUT_OF_MEMORY = 4,
    };
    
    struct MkuiResult {
        MkuiErrorCode code;
        const char* message;
    };
    
    // C API functions
    MkuiApp* mkui_app_new();
    void mkui_app_free(MkuiApp* app);
    MkuiResult mkui_app_add_view(MkuiApp* app, const char* class_name);
    MkuiResult mkui_app_add_text(MkuiApp* app, const char* content, const char* class_name);
    MkuiResult mkui_app_add_button(MkuiApp* app, const char* text, const char* class_name, int variant);
    MkuiResult mkui_app_run_console(MkuiApp* app);
    void mkui_free_error_message(char* message);
    const char* mkui_version();
    
    // Button variant constants
    extern const int MKUI_BUTTON_PRIMARY;
    extern const int MKUI_BUTTON_SECONDARY;
    extern const int MKUI_BUTTON_DESTRUCTIVE;
    extern const int MKUI_BUTTON_OUTLINE;
    extern const int MKUI_BUTTON_GHOST;
    extern const int MKUI_BUTTON_LINK;
}

namespace mkui {

/// Exception thrown when mkui operations fail
class MkuiException : public std::runtime_error {
public:
    explicit MkuiException(const std::string& message) 
        : std::runtime_error(message) {}
    
    explicit MkuiException(MkuiErrorCode code, const std::string& message)
        : std::runtime_error(message), error_code_(code) {}
    
    MkuiErrorCode code() const noexcept { return error_code_; }

private:
    MkuiErrorCode error_code_ = MKUI_RUNTIME_ERROR;
};

/// Button variant enumeration
enum class ButtonVariant {
    Primary = 0,
    Secondary = 1,
    Destructive = 2,
    Outline = 3,
    Ghost = 4,
    Link = 5
};

/// RAII wrapper for mkui applications
class App {
public:
    /// Create a new mkui application
    App() : app_(mkui_app_new()) {
        if (!app_) {
            throw MkuiException(MKUI_INITIALIZATION_FAILED, "Failed to initialize mkui application");
        }
    }
    
    /// Destructor - automatically cleans up resources
    ~App() {
        if (app_) {
            mkui_app_free(app_);
        }
    }
    
    // Delete copy constructor and assignment operator
    App(const App&) = delete;
    App& operator=(const App&) = delete;
    
    // Allow move construction and assignment
    App(App&& other) noexcept : app_(other.app_) {
        other.app_ = nullptr;
    }
    
    App& operator=(App&& other) noexcept {
        if (this != &other) {
            if (app_) {
                mkui_app_free(app_);
            }
            app_ = other.app_;
            other.app_ = nullptr;
        }
        return *this;
    }
    
    /// Add a view component with optional CSS class
    App& addView(const std::string& className = "") {
        checkResult(mkui_app_add_view(app_, className.empty() ? nullptr : className.c_str()));
        return *this;
    }
    
    /// Add a text component with content and optional CSS class
    App& addText(const std::string& content, const std::string& className = "") {
        checkResult(mkui_app_add_text(app_, content.c_str(), 
                                     className.empty() ? nullptr : className.c_str()));
        return *this;
    }
    
    /// Add a button component
    App& addButton(const std::string& text, 
                   ButtonVariant variant = ButtonVariant::Primary,
                   const std::string& className = "") {
        checkResult(mkui_app_add_button(app_, text.c_str(),
                                       className.empty() ? nullptr : className.c_str(),
                                       static_cast<int>(variant)));
        return *this;
    }
    
    /// Run the application (console mode)
    void runConsole() {
        checkResult(mkui_app_run_console(app_));
    }
    
    /// Get library version
    static std::string version() {
        return std::string(mkui_version());
    }

private:
    void checkResult(const MkuiResult& result) {
        if (result.code != MKUI_SUCCESS) {
            std::string message = result.message ? result.message : "Unknown error";
            if (result.message) {
                mkui_free_error_message(const_cast<char*>(result.message));
            }
            throw MkuiException(result.code, message);
        }
    }
    
    MkuiApp* app_;
};

/// Convenience function to create an App
inline std::unique_ptr<App> createApp() {
    return std::make_unique<App>();
}

} // namespace mkui