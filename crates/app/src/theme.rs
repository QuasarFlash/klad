use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Id, Margin, Rect, Response, Shadow,
    Stroke, Style, TextStyle, WidgetText,
    class::HasClasses as _,
    style::ScrollStyle,
    theme::{DefaultStyle, StyleProvider},
    vec2,
    widget_style::{ButtonStyle, CheckboxStyle, SeparatorStyle, StyleArgs, TextEditStyle, WidgetState},
};
use std::sync::Arc;

pub const CONTROL: f32 = 28.0;
pub const MENU_ROW: f32 = 24.0;
pub const ROW: f32 = CONTROL;
pub const GRAB: f32 = 12.0;

pub const SMALL: f32 = 11.0;
pub const LABEL: f32 = 12.0;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 15.0;
pub const DISPLAY: f32 = 28.0;
pub const WORDMARK: f32 = 44.0;

pub const ICON_TINY: f32 = 9.0;
pub const ICON_SMALL: f32 = 14.0;
pub const ICON: f32 = 16.0;
pub const ICON_LARGE: f32 = 24.0;

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub mod radius {
    pub const CONTROL: u8 = 4;
    pub const CARD: u8 = 8;
}

pub const HAIRLINE: f32 = 1.0;
pub const EASE: f32 = 0.12;

pub const BG: Color32 = Color32::from_gray(24);
pub const PANEL: Color32 = Color32::from_gray(32);
pub const CONTROL_FILL: Color32 = Color32::from_gray(43);
pub const HOVER: Color32 = Color32::from_gray(53);
pub const PRESSED: Color32 = Color32::from_gray(61);
pub const BORDER: Color32 = Color32::from_gray(52);
pub const SURROUND: [u8; 3] = [58, 58, 58];

pub const TEXT: Color32 = Color32::from_gray(230);
pub const WEAK: Color32 = Color32::from_gray(168);
pub const DISABLED: Color32 = Color32::from_gray(110);
pub const ON_ACCENT: Color32 = Color32::WHITE;

pub const ACCENT: Color32 = Color32::from_rgb(110, 168, 255);
pub const ACCENT_FILL: Color32 = Color32::from_rgb(45, 111, 214);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(52, 122, 228);
pub const ACCENT_PRESSED: Color32 = Color32::from_rgb(37, 96, 190);
pub const ACCENT_SUBTLE: Color32 = Color32::from_rgb(36, 56, 87);
pub const ACCENT_SUBTLE_HOVER: Color32 = Color32::from_rgb(42, 66, 102);

pub const DONE: Color32 = Color32::from_rgb(120, 200, 140);
pub const REFUSED: Color32 = Color32::from_rgb(232, 190, 110);
pub const ERROR: Color32 = Color32::from_rgb(240, 120, 120);

pub const MARK: Color32 = Color32::from_rgb(64, 196, 255);
pub const CHECK_LIGHT: Color32 = Color32::from_gray(240);
pub const CHECK_DARK: Color32 = Color32::from_gray(205);

pub const MEDIUM: &str = "medium";
pub const SEMIBOLD: &str = "semibold";
pub const WORDMARK_FAMILY: &str = "wordmark";

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn wordmark(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(WORDMARK_FAMILY.into()))
}

pub mod icon {
    pub const BRUSH: &str = "\u{E6F0}";
    pub const SUN: &str = "\u{E472}";
    pub const ADJUST: &str = "\u{E434}";
    pub const SPARKLE: &str = "\u{E6A2}";
    pub const RECTANGLE: &str = "\u{E3F0}";
    pub const MOUNTAINS: &str = "\u{E7AE}";
    pub const REPEAT: &str = "\u{E3F6}";
    pub const SCATTER: &str = "\u{E1FC}";
    pub const IMAGE: &str = "\u{E2CA}";
    pub const ERASER: &str = "\u{E21E}";
    pub const FILL: &str = "\u{E392}";
    pub const SHAPES: &str = "\u{E1DC}";
    pub const STROKES: &str = "\u{E69A}";
    pub const POINTS: &str = "\u{EB00}";
    pub const PEN: &str = "\u{E3AC}";
    pub const PREPARE: &str = "\u{E2DE}";
    pub const NOTE: &str = "\u{E3E2}";
    pub const EYEDROPPER: &str = "\u{E568}";
    pub const MINUS: &str = "\u{E32A}";
    pub const PLUS: &str = "\u{E3D4}";
    pub const HAND: &str = "\u{E298}";
    pub const ZOOM: &str = "\u{E30C}";
    pub const SEARCH: &str = "\u{E30C}";
    pub const TEXT_T: &str = "\u{E48A}";
    pub const DOWNLOAD: &str = "\u{E20C}";
    pub const FIT: &str = "\u{E1D0}";
    pub const TURN: &str = "\u{E036}";
    pub const BONE: &str = "\u{E7F2}";
    pub const UNDO: &str = "\u{E08A}";
    pub const REDO: &str = "\u{E08C}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const CLOSE: &str = "\u{E4F6}";
    pub const COPY: &str = "\u{E1CA}";
    pub const CUT: &str = "\u{EAE0}";
    pub const TO_FRONT: &str = "\u{E066}";
    pub const TO_BACK: &str = "\u{E05C}";
    pub const UNITE: &str = "\u{E87E}";
    pub const MERGE: &str = "\u{ED3E}";
    pub const CROP: &str = "\u{E1D4}";
    pub const MAGNET: &str = "\u{E680}";
    pub const RULER: &str = "\u{E6B8}";
    pub const PERSPECTIVE: &str = "\u{EBE6}";
    pub const STAR: &str = "\u{E46A}";
    pub const RENAME: &str = "\u{E7D8}";
    pub const DUPLICATE: &str = "\u{EDF6}";
    pub const DROP: &str = "\u{E210}";
    pub const SUBTRACT: &str = "\u{EBD6}";
    pub const INTERSECT: &str = "\u{E2D2}";
    pub const EXCLUDE: &str = "\u{E882}";
    pub const PASTE: &str = "\u{E198}";
    pub const SELECT_ALL: &str = "\u{E746}";
    pub const LASSO: &str = "\u{EDC6}";
    pub const PALETTE: &str = "\u{E6C8}";
    pub const FLIP_H: &str = "\u{ED6A}";
    pub const ALIGN_LEFT: &str = "\u{E50E}";
    pub const ALIGN_CENTRE_ACROSS: &str = "\u{E50A}";
    pub const ALIGN_RIGHT: &str = "\u{E510}";
    pub const ALIGN_TOP: &str = "\u{E512}";
    pub const ALIGN_CENTRE_DOWN: &str = "\u{E50C}";
    pub const ALIGN_BOTTOM: &str = "\u{E506}";
    pub const DISTRIBUTE_ACROSS: &str = "\u{E534}";
    pub const DISTRIBUTE_DOWN: &str = "\u{E536}";
    pub const FLIP_V: &str = "\u{ED6C}";
    pub const LAYERS: &str = "\u{E466}";
    pub const PIXELS: &str = "\u{E296}";
    pub const EYE: &str = "\u{E220}";
    pub const EYE_OFF: &str = "\u{E224}";
    pub const LOCK: &str = "\u{E2FA}";
    pub const CLIP: &str = "\u{E2D2}";
    pub const MASK: &str = "\u{E18C}";
    pub const ONION: &str = "\u{E62A}";
    pub const STEP_ON: &str = "\u{E13A}";
    pub const STEP_BACK: &str = "\u{E138}";
    pub const NEXT: &str = "\u{E5A6}";
    pub const PREVIOUS: &str = "\u{E5A4}";
    pub const NEW_DRAWING: &str = "\u{E236}";
    pub const LINK: &str = "\u{E2E2}";
    pub const UNLINK: &str = "\u{E2E4}";
    pub const INSERT: &str = "\u{E542}";
    pub const REMOVE: &str = "\u{ED4C}";
    pub const FLIP_FRAMES: &str = "\u{E0A0}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const LOOP: &str = "\u{E3F6}";
    pub const SETTINGS: &str = "\u{E270}";
    pub const RETIME: &str = "\u{E492}";
    pub const INBETWEEN: &str = "\u{EA9A}";
    pub const CLIP_BAR: &str = "\u{E8C2}";
    pub const EXPORT: &str = "\u{EAF0}";
    pub const KEYBOARD: &str = "\u{E2D8}";
    pub const SIDEBAR: &str = "\u{EC24}";
    pub const INSPECTOR: &str = "\u{E434}";
    pub const HISTORY: &str = "\u{E1A0}";
    pub const TIMELINE: &str = "\u{E792}";
    pub const NUMBERS: &str = "\u{E628}";
    pub const TYPED: &str = "\u{E2A2}";
    pub const LOG: &str = "\u{E2F0}";
    pub const UP: &str = "\u{E08E}";
    pub const DOWN: &str = "\u{E03E}";
    pub const LEFT: &str = "\u{E058}";
    pub const RIGHT: &str = "\u{E06C}";
    pub const TURN_LEFT: &str = "\u{E038}";
    pub const SWAP: &str = "\u{E83C}";
    pub const SIZE: &str = "\u{E18A}";
    pub const MORE: &str = "\u{E1FE}";
    pub const OPEN: &str = "\u{E136}";
    pub const FOLDED: &str = "\u{E13A}";
    pub const CHECK: &str = "\u{E182}";
    pub const WARNING: &str = "\u{E4E2}";
    pub const INFO: &str = "\u{E2CE}";
    pub const FILE: &str = "\u{E230}";
    pub const FILE_PLUS: &str = "\u{E236}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
    pub const SAVE: &str = "\u{E248}";
    pub const STACK_PLUS: &str = "\u{EDF6}";
    pub const CARDS: &str = "\u{E0F6}";
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    [
        ("inter", include_bytes!("../assets/Inter-Regular.ttf").as_slice()),
        ("inter-medium", include_bytes!("../assets/Inter-Medium.ttf").as_slice()),
        (
            "inter-semibold",
            include_bytes!("../assets/Inter-SemiBold.ttf").as_slice(),
        ),
        ("phosphor", include_bytes!("../assets/Phosphor.ttf").as_slice()),
    ]
    .into_iter()
    .for_each(|(name, bytes)| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    });
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let chain = |first: Option<&str>| -> Vec<String> {
        first
            .map(String::from)
            .into_iter()
            .chain(fallback.iter().cloned())
            .chain(std::iter::once("phosphor".to_string()))
            .collect()
    };
    fonts.families.insert(FontFamily::Proportional, chain(Some("inter")));
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), chain(Some("inter-medium")));
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), chain(Some("inter-semibold")));
    fonts
        .families
        .insert(FontFamily::Name(WORDMARK_FAMILY.into()), chain(None));
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("phosphor".into());
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(apply);
    ctx.add_widget_theme::<ButtonStyle>(Look);
    ctx.add_widget_theme::<TextEditStyle>(Look);
    ctx.add_widget_theme::<CheckboxStyle>(Look);
    ctx.add_widget_theme::<SeparatorStyle>(Look);
}

pub fn apply(style: &mut Style) {
    style.text_styles = [
        (TextStyle::Small, regular(SMALL)),
        (TextStyle::Body, regular(BODY)),
        (TextStyle::Button, medium(BODY)),
        (TextStyle::Monospace, FontId::monospace(LABEL)),
        (TextStyle::Heading, semibold(HEADING)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = vec2(space::SM, space::XS);
    s.button_padding = vec2(space::SM, space::XS);
    s.interact_size = vec2(36.0, CONTROL);
    s.icon_width = 16.0;
    s.icon_width_inner = 8.0;
    s.icon_spacing = space::SM;
    s.slider_width = 120.0;
    s.slider_rail_height = space::XS;
    s.combo_width = 140.0;
    s.window_margin = Margin::same(space::MD as i8);
    s.menu_margin = Margin::same(space::XS as i8);
    s.scroll = scroll();
    style.interaction.tooltip_delay = 0.35;
    style.interaction.show_tooltips_only_when_still = true;
    style.animation_time = EASE;
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(HAIRLINE, BORDER);
    v.window_corner_radius = CornerRadius::same(radius::CARD);
    v.menu_corner_radius = CornerRadius::same(radius::CARD);
    v.window_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(80),
    };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = BG;
    v.text_edit_bg_color = Some(BG);
    v.faint_bg_color = CONTROL_FILL;
    v.code_bg_color = BG;
    v.selection.bg_fill = ACCENT_SUBTLE;
    v.selection.stroke = Stroke::new(HAIRLINE, TEXT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = REFUSED;
    v.error_fg_color = ERROR;
    v.weak_text_color = Some(WEAK);
    v.override_text_color = None;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(HAIRLINE, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(HAIRLINE, TEXT);
    w.inactive.bg_fill = CONTROL_FILL;
    w.inactive.weak_bg_fill = CONTROL_FILL;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(HAIRLINE, TEXT);
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    w.active.bg_fill = PRESSED;
    w.active.weak_bg_fill = PRESSED;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.5, TEXT);
    w.open.bg_fill = HOVER;
    w.open.weak_bg_fill = HOVER;
    w.open.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(HAIRLINE, TEXT);
    [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ]
    .into_iter()
    .for_each(|w| {
        w.corner_radius = CornerRadius::same(radius::CONTROL);
        w.expansion = 0.0;
    });
}

pub const PRIMARY: &str = "klad::primary";
pub const GHOST: &str = "klad::ghost";
pub const CENTRED: &str = "klad::centred";

pub fn focus_ring() -> Stroke {
    Stroke::new(1.5, ACCENT)
}

#[derive(Clone, Copy)]
struct Look;

impl StyleProvider<ButtonStyle> for Look {
    fn style(&mut self, args: &StyleArgs<'_>) -> ButtonStyle {
        let mut look = StyleProvider::<ButtonStyle>::style(&mut DefaultStyle, args);
        let selected = args.classes.has_class(&egui::Button::CLASS_SELECTED);
        let primary = args.classes.has_class(PRIMARY);
        let ghost = args.classes.has_class(GHOST);
        let state = args.state;
        let in_menu = args
            .stack
            .iter()
            .any(|ui| ui.kind() == Some(egui::UiKind::Menu) && ui.is_root_ui());
        let layout = &mut look.atom_layout;
        if in_menu && layout.min_size.y > 0.0 {
            layout.min_size.y = MENU_ROW;
        }
        let (fill, text) = match (primary, selected, state) {
            (_, _, WidgetState::Noninteractive) => (layout.frame.fill, TEXT),
            (true, _, WidgetState::Hovered) => (ACCENT_HOVER, ON_ACCENT),
            (true, _, WidgetState::Active) => (ACCENT_PRESSED, ON_ACCENT),
            (true, _, _) => (ACCENT_FILL, ON_ACCENT),
            (false, true, WidgetState::Hovered) => (ACCENT_SUBTLE_HOVER, TEXT),
            (false, true, _) => (ACCENT_SUBTLE, TEXT),
            (false, false, WidgetState::Inactive) if ghost => (Color32::TRANSPARENT, TEXT),
            (false, false, _) => (layout.frame.fill, TEXT),
        };
        layout.frame.fill = fill;
        layout.text_style.font_id = medium(layout.text_style.font_id.size.max(BODY));
        if state != WidgetState::Noninteractive {
            layout.text_style.color = text;
        }
        if primary {
            layout.image_tint = ON_ACCENT;
        }
        if primary || args.classes.has_class(CENTRED) {
            layout.align2 = Some(egui::Align2::CENTER_CENTER);
        }
        look
    }
}

impl StyleProvider<TextEditStyle> for Look {
    fn style(&mut self, args: &StyleArgs<'_>) -> TextEditStyle {
        let mut look = StyleProvider::<TextEditStyle>::style(&mut DefaultStyle, args);
        let read_only = args.classes.has_class(&egui::TextEdit::CLASS_READ_ONLY);
        let stroke = match (read_only, args.state) {
            (true, _) => Stroke::NONE,
            (false, WidgetState::Active) => focus_ring(),
            (false, WidgetState::Hovered) => Stroke::new(HAIRLINE, PRESSED),
            _ => Stroke::new(HAIRLINE, BORDER),
        };
        look.atom_layout.frame = Frame {
            fill: if read_only { Color32::TRANSPARENT } else { BG },
            corner_radius: CornerRadius::same(radius::CONTROL),
            inner_margin: args.style.spacing.button_padding.into(),
            ..Default::default()
        }
        .apply_stroke_and_expansion_without_layout_shift(stroke, 0.0);
        look.hint_text_color = DISABLED;
        look
    }
}

impl StyleProvider<CheckboxStyle> for Look {
    fn style(&mut self, args: &StyleArgs<'_>) -> CheckboxStyle {
        let mut look = StyleProvider::<CheckboxStyle>::style(&mut DefaultStyle, args);
        look.checkbox_frame.stroke = match args.state {
            WidgetState::Active => focus_ring(),
            _ => Stroke::new(HAIRLINE, PRESSED),
        };
        look.check_stroke = Stroke::new(2.0, TEXT);
        look
    }
}

impl StyleProvider<SeparatorStyle> for Look {
    fn style(&mut self, args: &StyleArgs<'_>) -> SeparatorStyle {
        SeparatorStyle {
            spacing: space::SM,
            ..StyleProvider::<SeparatorStyle>::style(&mut DefaultStyle, args)
        }
    }
}

fn scroll() -> ScrollStyle {
    ScrollStyle {
        bar_width: 8.0,
        floating_width: 4.0,
        handle_min_length: CONTROL,
        bar_inner_margin: 2.0,
        bar_outer_margin: 2.0,
        dormant_handle_opacity: 0.0,
        active_handle_opacity: 0.35,
        interact_handle_opacity: 0.7,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.0,
        interact_background_opacity: 0.0,
        ..ScrollStyle::floating()
    }
}

pub fn shown_scroll() -> ScrollStyle {
    ScrollStyle {
        dormant_handle_opacity: 0.12,
        ..scroll()
    }
}

pub fn surround(rgb: [u8; 3]) -> [f32; 4] {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    [r, g, b, 1.0]
}

fn hinted_id() -> Id {
    Id::unique("klad hinted controls")
}

#[cfg(test)]
fn hint_texts_id() -> Id {
    Id::unique("klad hint texts")
}

#[cfg(test)]
fn hint_parts_id() -> Id {
    Id::unique("klad hint parts")
}

pub fn clear_hints(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp::<Vec<Rect>>(hinted_id(), Vec::new()));
    #[cfg(test)]
    ctx.data_mut(|d| {
        d.insert_temp::<Vec<(Rect, String)>>(hint_texts_id(), Vec::new());
        d.insert_temp::<Vec<(Rect, Rect, String)>>(hint_parts_id(), Vec::new());
    });
}

#[cfg(test)]
pub fn hinted(ctx: &egui::Context) -> Vec<Rect> {
    ctx.data(|d| d.get_temp::<Vec<Rect>>(hinted_id()).unwrap_or_default())
}

#[cfg(test)]
pub fn hints(ctx: &egui::Context) -> Vec<(Rect, String)> {
    ctx.data(|d| d.get_temp::<Vec<(Rect, String)>>(hint_texts_id()).unwrap_or_default())
}

#[cfg(test)]
pub fn hint_parts(ctx: &egui::Context) -> Vec<(Rect, Rect, String)> {
    ctx.data(|d| {
        d.get_temp::<Vec<(Rect, Rect, String)>>(hint_parts_id())
            .unwrap_or_default()
    })
}

pub fn held_menu<'a, R>(
    ui: &mut egui::Ui,
    atoms: impl egui::IntoAtoms<'a>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let (response, inner) = egui::containers::menu::MenuButton::new(atoms)
        .config(egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
        .ui(ui, add);
    egui::InnerResponse::new(inner.map(|i| i.inner), response)
}

pub fn select_all(ctx: &egui::Context, field: Id, text: &str) {
    let mut state = egui::text_edit::TextEditState::load(ctx, field).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
    state.store(ctx, field);
}

pub trait Hint {
    fn hint(self, text: impl Into<WidgetText>) -> Response;
}

impl Hint for Response {
    fn hint(self, text: impl Into<WidgetText>) -> Response {
        let rect = self.rect;
        let text = match text.into() {
            WidgetText::Text(words) => WidgetText::Text(crate::commands::local_keys(&words)),
            other => other,
        };
        #[cfg(test)]
        let seen = self.interact_rect;
        self.ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Vec<Rect>>(hinted_id()).push(rect);
            #[cfg(test)]
            d.get_temp_mut_or_default::<Vec<(Rect, String)>>(hint_texts_id())
                .push((rect, text.text().to_string()));
            #[cfg(test)]
            d.get_temp_mut_or_default::<Vec<(Rect, Rect, String)>>(hint_parts_id())
                .push((rect, seen, text.text().to_string()));
        });
        self.on_hover_text(text)
    }
}
