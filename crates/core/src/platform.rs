//! Platform services the embedding host provides to mkui.
//!
//! The OS clipboard and the native share sheet are host/platform
//! capabilities — `mkui-core` cannot reach them directly today, and once a
//! deployment target is sandboxed (issue #173's wgpu-host-embedding effort)
//! it must not try. [`PlatformHost`] is the contract: the embedding
//! application implements it against whatever OS API it has access to
//! (`web_sys::Clipboard` / `navigator.share` on web, `NSPasteboard` /
//! `UIActivityViewController` on Apple platforms, `arboard` + a custom share
//! sheet on desktop, …) and hands mkui a handle to it. mkui-core ships no
//! OS-touching implementation — only [`NoopPlatformHost`], a safe default
//! for headless targets and tests.
//!
//! There is no dedicated action-registry plumbing for this — none is
//! needed. `Button::on_press` already accepts an arbitrary `Fn()`, so an
//! application wires platform services by capturing a host handle in the
//! closure it passes to `on_press`:
//!
//! ```ignore
//! use std::rc::Rc;
//! use mkui_core::prelude::*;
//! use mkui_core::platform::{NoopPlatformHost, PlatformHost};
//!
//! let host: Rc<dyn PlatformHost> = Rc::new(NoopPlatformHost);
//! let button = Button::new("Copy link").on_press(move || {
//!     let _ = host.copy_to_clipboard("https://example.com");
//! });
//! ```
//!
//! mkui never sees the concrete host type and never calls an OS API itself —
//! the call happens inside application-authored code when the action fires.

use crate::error::MkuiError;

/// A native-share request. Every field is optional; a [`PlatformHost`]
/// implementation decides how to honor whichever subset is populated (for
/// example, a web host might build a `web_sys::ShareData`, ignoring fields
/// the share sheet doesn't support).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShareRequest {
    pub title: Option<String>,
    pub text: Option<String>,
    pub url: Option<String>,
}

impl ShareRequest {
    /// An empty share request. Prefer chaining [`title`](Self::title()),
    /// [`text`](Self::text()), and/or [`url`](Self::url()).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// Platform services an mkui host implements on behalf of the UI.
///
/// `mkui-core` deliberately ships no implementation of this trait beyond
/// [`NoopPlatformHost`] — per the crate's own contract-crate rule (no
/// `wasm-bindgen`, `web-sys`, or OS-facing dependencies), every real
/// implementation lives in host/application code. An action reaches the
/// clipboard or the share sheet only by calling through a `PlatformHost` the
/// host supplied; `mkui-core` makes no filesystem or network calls as part
/// of this contract.
pub trait PlatformHost {
    /// Copy `text` to the system clipboard.
    fn copy_to_clipboard(&self, text: &str) -> Result<(), MkuiError>;

    /// Open the OS-native share sheet for `request`.
    fn share(&self, request: ShareRequest) -> Result<(), MkuiError>;
}

/// A [`PlatformHost`] that performs no OS work and always succeeds.
///
/// This is the only [`PlatformHost`] `mkui-core` ships. It is the right
/// default for headless targets (CI, server-side rendering) and the natural
/// double for unit tests that exercise an action without a real clipboard or
/// share sheet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopPlatformHost;

impl PlatformHost for NoopPlatformHost {
    fn copy_to_clipboard(&self, _text: &str) -> Result<(), MkuiError> {
        Ok(())
    }

    fn share(&self, _request: ShareRequest) -> Result<(), MkuiError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn noop_platform_host_copy_to_clipboard_succeeds() {
        let host = NoopPlatformHost;
        assert!(host.copy_to_clipboard("hello").is_ok());
    }

    #[test]
    fn noop_platform_host_share_succeeds() {
        let host = NoopPlatformHost;
        assert!(host
            .share(ShareRequest::new().url("https://example.com"))
            .is_ok());
    }

    #[test]
    fn share_request_builder_retains_every_field() {
        let request = ShareRequest::new()
            .title("Check this out")
            .text("Found this useful")
            .url("https://example.com");

        assert_eq!(request.title.as_deref(), Some("Check this out"));
        assert_eq!(request.text.as_deref(), Some("Found this useful"));
        assert_eq!(request.url.as_deref(), Some("https://example.com"));
    }

    /// A test-only `PlatformHost` that records every call instead of
    /// touching the OS. Demonstrates acceptance criterion #2 (an interface
    /// usable in unit tests, no real clipboard/share sheet involved) and
    /// backs the round-trip test below.
    #[derive(Default)]
    struct RecordingPlatformHost {
        clipboard_calls: RefCell<Vec<String>>,
        share_calls: RefCell<Vec<ShareRequest>>,
    }

    impl PlatformHost for RecordingPlatformHost {
        fn copy_to_clipboard(&self, text: &str) -> Result<(), MkuiError> {
            self.clipboard_calls.borrow_mut().push(text.to_string());
            Ok(())
        }

        fn share(&self, request: ShareRequest) -> Result<(), MkuiError> {
            self.share_calls.borrow_mut().push(request);
            Ok(())
        }
    }

    #[test]
    fn ui_action_round_trips_through_a_host_provided_implementation() {
        use crate::components::{Button, Mkui};

        let host = Rc::new(RecordingPlatformHost::default());
        let host_in = Rc::clone(&host);

        let app = Mkui::new().child(Button::new("Copy link").on_press(move || {
            host_in
                .copy_to_clipboard("https://example.com/post/42")
                .expect("noop/test host must not fail");
        }));

        let root = app.tree().get(app.tree().root()).unwrap();
        let mkui_runtime::NodeKind::Button(button) =
            &app.tree().get(root.children[0]).unwrap().kind
        else {
            panic!("expected Button");
        };
        let action = button.on_press.expect("handler should be registered");
        app.tree().actions().fire(action);

        assert_eq!(
            *host.clipboard_calls.borrow(),
            vec!["https://example.com/post/42".to_string()],
            "the action must reach the host through `PlatformHost`, not an OS API"
        );
        assert!(host.share_calls.borrow().is_empty());
    }
}
