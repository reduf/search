use imgui::*;

pub struct HotkeysWindow {
    opened: bool,
}

impl HotkeysWindow {
    pub fn new() -> Self {
        Self { opened: false }
    }

    pub fn open(&mut self, opened: bool) {
        self.opened = opened;
    }

    pub fn toggle_open(&mut self) {
        self.open(!self.opened);
    }

    pub fn draw_hotkeys_help(&mut self, ui: &Ui) {
        if !self.opened {
            return;
        }

        let display_size = ui.io().display_size;
        let settings_window_size = [750.0, 562.0];
        let pos_x = (display_size[0] / 2.0) - (settings_window_size[0] / 2.0);
        let pos_y = (display_size[1] / 2.0) - (settings_window_size[1] / 2.0);

        let window = ui
            .window("Hotkeys")
            .size(settings_window_size, Condition::Appearing)
            .position([pos_x, pos_y], Condition::Appearing)
            .menu_bar(false)
            .collapsible(false)
            .opened(&mut self.opened);

        let hotkeys = [
            ("F1", "Close/Open this window."),
            ("ESC", "Cancel search."),
            ("Ctrl+T", "Creates a new tab."),
            ("Ctrl+Shift+T", "Duplicate current tab."),
            ("Ctrl+W", "Close current tab."),
            ("Ctrl+PageUp", "Rotate current tab to the left."),
            ("Ctrl+PageDown", "Rotate current tab to the right."),
            ("F4", "Open selected files with your configured editor."),
        ];

        window.build(|| {
            ui.text("Hotkeys");
            if let Some(_t) = ui.begin_table_with_flags("tab-hotkeys-layout", 2, TableFlags::SIZING_FIXED_FIT) {
                ui.table_setup_column_with(TableColumnSetup { name: "##hotkeys", flags: TableColumnFlags::WIDTH_FIXED, init_width_or_weight: 0.0, user_id: Id::default() });
                ui.table_setup_column_with(TableColumnSetup { name: "##description", flags: TableColumnFlags::WIDTH_STRETCH, init_width_or_weight: 0.0, user_id: Id::default() });
                ui.table_next_row();

                for (hotkey, help) in &hotkeys {
                    ui.table_next_column();
                    ui.text(hotkey);
                    ui.table_next_column();
                    ui.text(help);
                }
            }
        });
    }
}
