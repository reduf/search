use arboard::Clipboard;
use imgui::ClipboardBackend;

pub struct ClipboardSupport(pub Clipboard);

pub fn init() -> Option<ClipboardSupport> {
    Clipboard::new().ok().map(ClipboardSupport)
}

impl ClipboardBackend for ClipboardSupport {
    fn get(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }
    fn set(&mut self, text: &str) {
        // ignore errors?
        let _ = self.0.set_text(text.to_owned());
    }
}
