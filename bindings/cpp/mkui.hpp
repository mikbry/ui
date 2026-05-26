#pragma once

// Use the cbindgen-generated C header directly — the prior hand-maintained
// forward declarations drifted from the FFI when the Sprint 4 handle API
// landed. Include the generated header instead.
#include "../../crates/mkui-c/include/mkui_c.h"

#include <functional>
#include <memory>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <utility>

namespace mkui {

/// Exception thrown when mkui operations fail.
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

/// Button variant enumeration — wire-compatible with `MKUI_BUTTON_*`.
enum class ButtonVariant : int {
    Primary = 0,
    Secondary = 1,
    Destructive = 2,
    Outline = 3,
    Ghost = 4,
    Link = 5,
};

/// Text variant enumeration — wire-compatible with `MKUI_TEXT_*`.
enum class TextVariant : int {
    Body = 0,
    Heading1 = 1,
    Heading2 = 2,
    Heading3 = 3,
    Caption = 4,
    Label = 5,
    Code = 6,
};

/// Sentinel value for "no callback" on a button.
inline constexpr MkuiActionId kNoAction{UINT32_MAX, UINT32_MAX};

/// Sentinel returned when a child-constructor call fails (invalid class,
/// stale parent, etc.).
inline constexpr MkuiNodeId kInvalidNode{UINT32_MAX, UINT32_MAX};

/// Strongly-typed wrapper around `MkuiNodeId`. Construction is implicit so
/// fluent chaining feels natural; equality compares both index and
/// generation (the use-after-free guard).
class NodeId {
public:
    NodeId() = default;
    NodeId(MkuiNodeId raw) : raw_(raw) {}

    bool valid() const noexcept {
        return raw_.index != kInvalidNode.index || raw_.generation != kInvalidNode.generation;
    }
    MkuiNodeId raw() const noexcept { return raw_; }
    uint32_t index() const noexcept { return raw_.index; }
    uint32_t generation() const noexcept { return raw_.generation; }

    bool operator==(const NodeId& other) const noexcept {
        return raw_.index == other.raw_.index && raw_.generation == other.raw_.generation;
    }
    bool operator!=(const NodeId& other) const noexcept { return !(*this == other); }

private:
    MkuiNodeId raw_ = kInvalidNode;
};

/// RAII wrapper for mkui applications using the Sprint 4 handle-based API.
class App {
public:
    /// Create a new mkui application.
    App() : app_(mkui_app_new()) {
        if (!app_) {
            throw MkuiException(MKUI_INITIALIZATION_FAILED,
                                "Failed to initialize mkui application");
        }
    }

    ~App() {
        if (app_) {
            mkui_app_free(app_);
        }
    }

    // No copy.
    App(const App&) = delete;
    App& operator=(const App&) = delete;

    // Move-only.
    App(App&& other) noexcept
        : app_(other.app_), callbacks_(std::move(other.callbacks_)) {
        other.app_ = nullptr;
    }
    App& operator=(App&& other) noexcept {
        if (this != &other) {
            if (app_) {
                mkui_app_free(app_);
            }
            app_ = other.app_;
            callbacks_ = std::move(other.callbacks_);
            other.app_ = nullptr;
        }
        return *this;
    }

    /// Root node — children attach under this or one of its descendants.
    NodeId root() const { return NodeId(mkui_app_root(app_)); }

    /// Append a `View` under `parent`.
    NodeId viewChild(NodeId parent, const std::string& className = "") {
        return NodeId(mkui_app_view_child(app_, parent.raw(),
                                          className.empty() ? nullptr : className.c_str()));
    }

    /// Append a `Text` under `parent`.
    NodeId textChild(NodeId parent, const std::string& content,
                     TextVariant variant = TextVariant::Body,
                     const std::string& className = "") {
        return NodeId(mkui_app_text_child(
            app_, parent.raw(), content.c_str(), static_cast<int>(variant),
            className.empty() ? nullptr : className.c_str()));
    }

    /// Append a `Button` under `parent`.
    NodeId buttonChild(NodeId parent, const std::string& label,
                       ButtonVariant variant = ButtonVariant::Primary,
                       const std::string& className = "",
                       MkuiActionId onPress = kNoAction) {
        return NodeId(mkui_app_button_child(
            app_, parent.raw(), label.c_str(), static_cast<int>(variant),
            className.empty() ? nullptr : className.c_str(), onPress));
    }

    /// Register a C++ callable as an action callback. The returned id
    /// is what `buttonChild` accepts. The callable is kept alive on
    /// the C++ side for the lifetime of the `App`.
    MkuiActionId registerCallback(std::function<void()> fn) {
        MkuiActionId id = mkui_app_register_callback(app_, &trampoline,
                                                     reinterpret_cast<void*>(next_callback_id_));
        callbacks_[next_callback_id_] = std::move(fn);
        ++next_callback_id_;
        return id;
    }

    /// Run the real interactive console backend (Sprint 4 restored
    /// the actual backend invocation; the round-7 stub `println` is gone).
    void runConsole() { checkResult(mkui_app_run_console(app_)); }

    /// Canonical JSON snapshot of the current tree — what the parity
    /// tests compare against the Rust reference.
    std::string snapshotJson() const {
        char* raw = mkui_app_snapshot_json(app_);
        if (!raw) {
            throw MkuiException(MKUI_RUNTIME_ERROR, "snapshot returned null");
        }
        std::string s(raw);
        mkui_free_error_message(raw);
        return s;
    }

    /// Library version.
    static std::string version() { return std::string(mkui_version()); }

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

    static void trampoline(void* user_data) {
        // `user_data` is the small integer key into the callback table
        // we passed at registration time.
        auto key = reinterpret_cast<uintptr_t>(user_data);
        if (auto* app = current_app_for_trampoline()) {
            auto it = app->callbacks_.find(key);
            if (it != app->callbacks_.end()) {
                it->second();
            }
        }
    }

    // Single-app-per-thread shim: the C trampoline can't capture state,
    // so we keep a thread-local pointer to "the app whose run_console
    // is on the stack." Multi-app callers can compose by chaining apps,
    // not running them concurrently (consistent with the runtime's
    // single-threaded invariant).
    static App*& current_app_for_trampoline() {
        thread_local App* current = nullptr;
        return current;
    }

    MkuiApp* app_;
    std::unordered_map<uintptr_t, std::function<void()>> callbacks_;
    uintptr_t next_callback_id_ = 1;
};

/// Convenience factory.
inline std::unique_ptr<App> createApp() { return std::make_unique<App>(); }

} // namespace mkui
