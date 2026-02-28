use egui::{ColorImage, TextureHandle, TextureOptions};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use system_tray::client::{Client, Event, UpdateEvent};
use system_tray::item::{IconPixmap, StatusNotifierItem};
use system_tray::menu::TrayMenu;

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

/// Cached tray item for rendering
#[derive(Debug, Clone)]
struct TrayItem {
    id: String,
    title: Option<String>,
    icon_name: Option<String>,
    icon_pixmap: Option<Vec<IconPixmap>>,
    status: String,
    menu: Option<TrayMenu>,
}

impl TrayItem {
    fn from_sni(id: String, item: &StatusNotifierItem) -> Self {
        Self {
            id,
            title: item.title.clone(),
            icon_name: item.icon_name.clone(),
            icon_pixmap: item.icon_pixmap.clone(),
            status: format!("{:?}", item.status),
            menu: None,
        }
    }

    fn display_name(&self) -> &str {
        self.title
            .as_deref()
            .unwrap_or_else(|| self.icon_name.as_deref().unwrap_or(&self.id))
    }
}

/// Shared state between the async tray client and the egui module
type TrayState = Arc<Mutex<TrayStateInner>>;

struct TrayStateInner {
    items: HashMap<String, TrayItem>,
    /// Texture cache: item_id -> texture handle
    textures: HashMap<String, TextureHandle>,
    /// Whether we need to rebuild textures
    textures_dirty: bool,
}

impl TrayStateInner {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
            textures: HashMap::new(),
            textures_dirty: false,
        }
    }
}

pub struct TrayModule {
    tray_state: TrayState,
    show_popup: bool,
    popup_item_id: Option<String>,
}

impl TrayModule {
    pub fn new(egui_ctx: egui::Context, rt_handle: tokio::runtime::Handle) -> Self {
        let tray_state: TrayState = Arc::new(Mutex::new(TrayStateInner::new()));

        // Spawn tray client on the tokio runtime from main().
        // We use the captured Handle because eframe runs us on a non-tokio thread.
        // zbus (used by system-tray) requires a multi-thread runtime with a reactor.
        let state_clone = Arc::clone(&tray_state);
        let ctx_clone = egui_ctx.clone();
        rt_handle.spawn(async move {
            run_tray_client(state_clone, ctx_clone).await;
        });

        Self {
            tray_state,
            show_popup: false,
            popup_item_id: None,
        }
    }
}

/// Run the system-tray client, forwarding events to shared state
async fn run_tray_client(tray_state: TrayState, egui_ctx: egui::Context) {
    let client = match Client::new().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create system-tray client: {}", e);
            return;
        }
    };

    info!("System tray client started");

    let mut tray_rx = client.subscribe();

    // Load initial items
    {
        let initial = client.items();
        if let Ok(items_map) = initial.lock() {
            let mut state = tray_state.lock().unwrap();
            for (addr, (item, menu)) in items_map.iter() {
                let mut tray_item = TrayItem::from_sni(addr.clone(), item);
                tray_item.menu = menu.clone();
                info!("Initial tray item: {} ({})", tray_item.display_name(), addr);
                state.items.insert(addr.clone(), tray_item);
            }
            state.textures_dirty = true;
        }
        egui_ctx.request_repaint();
    }

    loop {
        match tray_rx.recv().await {
            Ok(event) => {
                let mut state = tray_state.lock().unwrap();
                match event {
                    Event::Add(addr, item) => {
                        info!("Tray item added: {}", addr);
                        let tray_item = TrayItem::from_sni(addr.clone(), &item);
                        state.items.insert(addr, tray_item);
                        state.textures_dirty = true;
                    }
                    Event::Update(addr, update) => {
                        debug!("Tray item updated: {}", addr);
                        if let Some(item) = state.items.get_mut(&addr) {
                            match update {
                                UpdateEvent::Title(title) => {
                                    item.title = title;
                                }
                                UpdateEvent::Icon { icon_name, icon_pixmap } => {
                                    if icon_name.is_some() {
                                        item.icon_name = icon_name;
                                    }
                                    if icon_pixmap.is_some() {
                                        item.icon_pixmap = icon_pixmap;
                                        state.textures_dirty = true;
                                    }
                                }
                                UpdateEvent::Status(status) => {
                                    item.status = format!("{:?}", status);
                                }
                                UpdateEvent::Tooltip(_tooltip) => {}
                                UpdateEvent::Menu(menu) => {
                                    item.menu = Some(menu);
                                }
                                UpdateEvent::MenuDiff(diffs) => {
                                    // Apply diffs to existing menu if present
                                    if let Some(ref mut menu) = item.menu {
                                        apply_menu_diffs(menu, &diffs);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Event::Remove(addr) => {
                        info!("Tray item removed: {}", addr);
                        state.items.remove(&addr);
                        state.textures.remove(&addr);
                    }
                }
                drop(state);
                egui_ctx.request_repaint();
            }
            Err(e) => {
                warn!("Tray event channel error: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Apply menu diffs to an existing TrayMenu
fn apply_menu_diffs(menu: &mut TrayMenu, diffs: &[system_tray::menu::MenuDiff]) {
    for diff in diffs {
        apply_diff_to_items(&mut menu.submenus, diff);
    }
}

fn apply_diff_to_items(
    items: &mut Vec<system_tray::menu::MenuItem>,
    diff: &system_tray::menu::MenuDiff,
) {
    for item in items.iter_mut() {
        if item.id == diff.id {
            // Apply updates
            if let Some(ref label) = diff.update.label {
                item.label = label.clone();
            }
            if let Some(enabled) = diff.update.enabled {
                item.enabled = enabled;
            }
            if let Some(visible) = diff.update.visible {
                item.visible = visible;
            }
            if let Some(ref toggle_state) = diff.update.toggle_state {
                item.toggle_state = toggle_state.clone();
            }
            return;
        }
        // Recurse into submenus
        apply_diff_to_items(&mut item.submenu, diff);
    }
}

/// Convert ARGB pixel data from IconPixmap to egui ColorImage
fn pixmap_to_color_image(pixmap: &IconPixmap) -> Option<ColorImage> {
    let w = pixmap.width as usize;
    let h = pixmap.height as usize;
    let expected_bytes = w * h * 4;

    if pixmap.pixels.len() < expected_bytes || w == 0 || h == 0 {
        return None;
    }

    let mut rgba = Vec::with_capacity(w * h * 4);

    // IconPixmap is in ARGB32 network byte order (big-endian)
    for chunk in pixmap.pixels.chunks_exact(4) {
        let a = chunk[0];
        let r = chunk[1];
        let g = chunk[2];
        let b = chunk[3];
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(a);
    }

    Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

/// Get the best pixmap (closest to desired size, prefer larger)
fn best_pixmap(pixmaps: &[IconPixmap], target_size: i32) -> Option<&IconPixmap> {
    pixmaps
        .iter()
        .filter(|p| p.width > 0 && p.height > 0 && !p.pixels.is_empty())
        .min_by_key(|p| (p.width - target_size).abs())
}

impl BarModule for TrayModule {
    fn id(&self) -> &str {
        "tray"
    }

    fn name(&self) -> &str {
        "System Tray"
    }

    fn has_popup(&self) -> bool {
        true
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, _state: &mut AppState, _anim: &mut AnimationState) {
        let mut state = self.tray_state.lock().unwrap();

        if state.items.is_empty() {
            return;
        }

        // Rebuild textures if needed
        if state.textures_dirty {
            let ctx = ui.ctx().clone();
            let mut new_textures = HashMap::new();

            for (addr, item) in &state.items {
                if let Some(ref pixmaps) = item.icon_pixmap {
                    if let Some(pixmap) = best_pixmap(pixmaps, 22) {
                        if let Some(image) = pixmap_to_color_image(pixmap) {
                            let texture = ctx.load_texture(
                                format!("tray_{}", addr),
                                image,
                                TextureOptions::LINEAR,
                            );
                            new_textures.insert(addr.clone(), texture);
                        }
                    }
                }
            }

            state.textures = new_textures;
            state.textures_dirty = false;
        }

        ui.separator();

        // Render each tray item as a small button
        let item_ids: Vec<String> = state.items.keys().cloned().collect();
        for addr in &item_ids {
            let item = &state.items[addr];
            let display = item.display_name().to_string();

            let resp = if let Some(texture) = state.textures.get(addr) {
                let size = egui::vec2(18.0, 18.0);
                let img = egui::Image::new(texture).fit_to_exact_size(size);
                ui.add(egui::ImageButton::new(img))
            } else {
                // Fallback: text button with first char or icon name
                let label = display.chars().next().unwrap_or('?').to_string();
                ui.add(egui::Button::new(
                    egui::RichText::new(label).color(colors::TEXT_SUBTLE),
                ))
            };

            if resp.clicked() {
                self.show_popup = true;
                self.popup_item_id = Some(addr.clone());
            }

            if resp.secondary_clicked() {
                self.show_popup = true;
                self.popup_item_id = Some(addr.clone());
            }

            resp.on_hover_text(&display);
        }
    }

    fn render_popup(&mut self, ctx: &egui::Context, _state: &mut AppState) {
        if !self.show_popup {
            return;
        }

        let Some(ref item_id) = self.popup_item_id else {
            return;
        };

        let tray_state = self.tray_state.lock().unwrap();
        let Some(item) = tray_state.items.get(item_id) else {
            self.show_popup = false;
            return;
        };

        let menu = item.menu.clone();
        let title = item.display_name().to_string();
        drop(tray_state);

        let mut open = true;
        egui::Window::new(format!("📦 {}", title))
            .collapsible(false)
            .resizable(false)
            .default_width(220.0)
            .open(&mut open)
            .show(ctx, |ui| {
                if let Some(ref menu) = menu {
                    render_menu_items(ui, &menu.submenus);
                } else {
                    ui.label(egui::RichText::new("No menu available").color(colors::TEXT_SUBTLE));
                }
            });

        if !open {
            self.show_popup = false;
            self.popup_item_id = None;
        }
    }
}

/// Render menu items recursively
fn render_menu_items(ui: &mut egui::Ui, items: &[system_tray::menu::MenuItem]) {
    use system_tray::menu::{MenuType, ToggleState, ToggleType};

    for item in items {
        if !item.visible {
            continue;
        }

        match item.menu_type {
            MenuType::Separator => {
                ui.separator();
            }
            MenuType::Standard => {
                let label_text = item.label.as_deref().unwrap_or("(untitled)");
                // Strip underscores used for mnemonics
                let clean_label = label_text.replace('_', "");

                let prefix = match item.toggle_type {
                    ToggleType::Checkmark => {
                        if matches!(item.toggle_state, ToggleState::On) {
                            "✓ "
                        } else {
                            "  "
                        }
                    }
                    ToggleType::Radio => {
                        if matches!(item.toggle_state, ToggleState::On) {
                            "● "
                        } else {
                            "○ "
                        }
                    }
                    ToggleType::CannotBeToggled => "",
                };

                let text = format!("{}{}", prefix, clean_label);

                let color = if item.enabled {
                    colors::TEXT
                } else {
                    colors::TEXT_SUBTLE
                };

                let resp = ui.add_enabled(
                    item.enabled,
                    egui::Button::new(egui::RichText::new(text).color(color))
                        .frame(false)
                        .min_size(egui::vec2(ui.available_width(), 0.0)),
                );

                if resp.clicked() {
                    debug!("Tray menu item clicked: id={}", item.id);
                    // Menu activation would require the async client reference
                    // For now, log the click - full activation will be connected
                    // when we pass an ActivateRequest sender to the module
                }

                // Render sub-items
                if !item.submenu.is_empty() {
                    ui.indent(format!("submenu_{}", item.id), |ui| {
                        render_menu_items(ui, &item.submenu);
                    });
                }
            }
        }
    }
}
