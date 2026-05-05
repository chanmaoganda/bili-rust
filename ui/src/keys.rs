use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

/// Owns a `keydown` listener registered on `window` and removes it on Drop.
/// Return one of these from a Leptos `Effect` to scope the listener to the
/// component's lifetime.
pub struct KeydownGuard {
    window: web_sys::Window,
    closure: Closure<dyn FnMut(KeyboardEvent)>,
}

impl Drop for KeydownGuard {
    fn drop(&mut self) {
        let _ = self
            .window
            .remove_event_listener_with_callback("keydown", self.closure.as_ref().unchecked_ref());
    }
}

impl KeydownGuard {
    /// Register the given handler on `window`. Returns `None` if there is no
    /// window (e.g. SSR).
    pub fn install<F: FnMut(KeyboardEvent) + 'static>(handler: F) -> Option<Self> {
        let window = web_sys::window()?;
        let closure = Closure::<dyn FnMut(KeyboardEvent)>::new(handler);
        let _ =
            window.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
        Some(KeydownGuard { window, closure })
    }
}

/// True if the focused element is a text input, textarea, or contenteditable —
/// in which case keyboard shortcuts must not hijack the keystroke.
pub fn is_typing(doc: &Document) -> bool {
    let Some(active) = doc.active_element() else {
        return false;
    };
    active.dyn_ref::<HtmlInputElement>().is_some()
        || active.dyn_ref::<HtmlTextAreaElement>().is_some()
        || active
            .dyn_ref::<HtmlElement>()
            .map(|e| e.is_content_editable())
            .unwrap_or(false)
}
