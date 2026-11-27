use crate::theme::{self, Hint, radius, space};
use egui::{
    Button, CornerRadius, Frame, InnerResponse, Margin, Response, RichText, Stroke, TextEdit, Ui, WidgetText,
    class::HasClasses as _, text::LayoutJob, vec2,
};

pub fn primary_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::PRIMARY)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn secondary_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::CENTRED)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn ghost_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    Button::new(text)
        .with_class(theme::GHOST)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn icon_button(ui: &mut Ui, glyph: &str, hint: impl Into<WidgetText>) -> Response {
    ui.add(
        Button::new(RichText::new(glyph).size(theme::ICON))
            .with_class(theme::GHOST)
            .min_size(vec2(theme::CONTROL, theme::CONTROL)),
    )
    .hint(hint)
}

pub fn quiet_icon_button(ui: &mut Ui, glyph: &str, hint: impl Into<WidgetText>) -> Response {
    let id = ui.next_auto_id();
    let lit = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let t = ui.ctx().animate_bool_with_time(id.with("lit"), lit, theme::EASE);
    let colour = theme::WEAK.lerp_to_gamma(theme::TEXT, t);
    ui.add(
        Button::new(RichText::new(glyph).size(theme::ICON).color(colour))
            .with_class(theme::GHOST)
            .min_size(vec2(theme::CONTROL, theme::CONTROL)),
    )
    .hint(hint)
}

pub fn field(text: &mut String, width: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .desired_width(width)
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(0.0, theme::CONTROL))
}

pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str, &str)]) -> Option<T> {
    segmented_in(ui, current, options, None)
}

pub fn segmented_wide<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str, &str)]) -> Option<T> {
    let each = ui.available_width() / options.len().max(1) as f32;
    segmented_in(ui, current, options, Some(each))
}

fn segmented_in<T: Copy + PartialEq>(
    ui: &mut Ui,
    current: T,
    options: &[(T, &str, &str)],
    each: Option<f32>,
) -> Option<T> {
    let mut picked = None;
    Frame::new()
        .fill(theme::CONTROL_FILL)
        .corner_radius(CornerRadius::same(radius::CONTROL))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                options.iter().for_each(|(value, label, hint)| {
                    let on = *value == current;
                    let button = Button::selectable(on, *label)
                        .with_class(theme::GHOST)
                        .with_class(theme::CENTRED)
                        .min_size(vec2(each.unwrap_or(0.0), theme::CONTROL));
                    if ui.add(button).hint(*hint).clicked() && !on {
                        picked = Some(*value);
                    }
                });
            });
        });
    picked
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame().show(ui, add)
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::same(space::LG as i8))
}

pub fn heading_text(glyph: &str, text: &str, size: f32) -> WidgetText {
    let mut job = LayoutJob::default();
    if !glyph.is_empty() {
        job.append(
            &format!("{glyph} "),
            0.0,
            egui::TextFormat::simple(theme::regular(size), theme::WEAK),
        );
    }
    job.append(text, 0.0, egui::TextFormat::simple(theme::semibold(size), theme::TEXT));
    WidgetText::LayoutJob(job.into())
}

pub fn section_header(ui: &mut Ui, glyph: &str, text: &str) -> Response {
    ui.label(heading_text(glyph, text, theme::HEADING))
}

pub fn heading_row(ui: &mut Ui, glyph: &str, text: &str, beside: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(theme::CONTROL);
        section_header(ui, glyph, text);
        ui.add_space(space::MD);
        beside(ui);
    });
}

pub fn large_primary_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    primary_button(text)
        .with_class(theme::LARGE)
        .min_size(vec2(0.0, theme::CONTROL_LARGE))
}

pub fn large_ghost_button<'a>(text: impl Into<WidgetText>) -> Button<'a> {
    ghost_button(text)
        .with_class(theme::LARGE)
        .with_class(theme::CENTRED)
        .min_size(vec2(0.0, theme::CONTROL_LARGE))
}

pub const ROW_BUTTON: f32 = 112.0;

pub fn row_button<'a>(text: &str, filled: bool) -> Button<'a> {
    let words = RichText::new(text).size(theme::CALLOUT);
    let button = match filled {
        true => secondary_button(words),
        false => ghost_button(words).with_class(theme::CENTRED),
    };
    button
        .min_size(vec2(ROW_BUTTON, theme::CONTROL_ROW))
        .corner_radius(radius::CARD)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG))
}

pub fn app_icon(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("klad app icon");
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(texture);
    }
    let picture = crate::home::window_icon()?;
    let image =
        egui::ColorImage::from_rgba_unmultiplied([picture.width as usize, picture.height as usize], &picture.rgba);
    let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    let texture = ctx.load_texture("klad icon", image, options);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

pub const LIST_ROW: f32 = 76.0;
pub const LIST_PAD: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub first: bool,
    pub last: bool,
}

impl Place {
    pub fn of(index: usize, count: usize) -> Self {
        Self {
            first: index == 0,
            last: index + 1 == count,
        }
    }
}

pub fn list_panel<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER))
        .corner_radius(CornerRadius::same(radius::PANEL))
        .show(ui, |ui| {
            ui.set_width(width - 2.0 * theme::HAIRLINE);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

fn list_frame(ui: &mut Ui, place: Place, fill: egui::Color32) -> Frame {
    if !place.first {
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::HAIRLINE), egui::Sense::hover());
        ui.painter().rect_filled(line, 0.0, theme::BORDER);
    }
    let inner = radius::PANEL - theme::HAIRLINE as u8;
    let top = if place.first { inner } else { 0 };
    let bottom = if place.last { inner } else { 0 };
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius {
            nw: top,
            ne: top,
            sw: bottom,
            se: bottom,
        })
        .inner_margin(Margin::symmetric(LIST_PAD as i8, 0))
}

pub fn list_row<R>(ui: &mut Ui, place: Place, fill: egui::Color32, gap: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let tall = LIST_ROW - if place.first { 0.0 } else { theme::HAIRLINE };
    list_frame(ui, place, fill)
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                vec2(width, tall),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(vec2(width, tall));
                    ui.spacing_mut().item_spacing.x = gap;
                    add(ui)
                },
            )
            .inner
        })
        .inner
}

pub fn list_section<R>(ui: &mut Ui, place: Place, add: impl FnOnce(&mut Ui) -> R) -> R {
    list_frame(ui, place, egui::Color32::TRANSPARENT)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(space::LG);
            let inner = add(ui);
            ui.add_space(space::LG);
            inner
        })
        .inner
}

pub fn bar_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(text).color(theme::WEAK))
}

pub fn form_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(text).font(theme::medium(theme::BODY)).color(theme::WEAK))
}

pub fn secondary_text(text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::WEAK)
}

pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text).size(theme::LABEL).color(theme::WEAK)
}

pub fn path_field(ui: &mut Ui, text: &mut String, width: f32) -> Response {
    let id = ui.next_auto_id().with("path");
    let editing = ui.memory(|m| m.has_focus(id) || m.data.get_temp::<bool>(id).unwrap_or(false));
    if editing {
        let response = ui.add(field(text, width).id(id));
        if response.lost_focus() {
            ui.memory_mut(|m| m.data.remove::<bool>(id));
        }
        return response;
    }
    let (rect, response) = ui.allocate_exact_size(vec2(width, theme::CONTROL), egui::Sense::click());
    let shown = text.clone();
    response.widget_info(|| egui::WidgetInfo::text_edit(ui.is_enabled(), shown.clone(), shown.clone(), ""));
    let edge = match response.hovered() {
        true => theme::PRESSED,
        false => theme::BORDER,
    };
    ui.painter().rect(
        rect,
        radius::CONTROL,
        theme::BG,
        Stroke::new(theme::HAIRLINE, edge),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink2(ui.spacing().button_padding);
    let galley = from_the_end(ui, text, inner.width());
    let at = egui::pos2(inner.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, theme::TEXT);
    if response.clicked() {
        ui.memory_mut(|m| {
            m.data.insert_temp(id, true);
            m.request_focus(id);
        });
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
                text.chars().count(),
            ))));
        state.store(ui.ctx(), id);
    }
    response.on_hover_cursor(egui::CursorIcon::Text)
}

fn from_the_end(ui: &Ui, text: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    let lay = |s: String| {
        WidgetText::from(s).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
    };
    let whole = lay(text.to_string());
    if whole.size().x <= width {
        return whole;
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .map(|k| lay(format!("\u{2026}{}", chars[k..].iter().collect::<String>())))
        .find(|g| g.size().x <= width)
        .unwrap_or_else(|| lay("\u{2026}".into()))
}

pub fn chip(ui: &mut Ui, selected: bool, text: impl Into<WidgetText>) -> Response {
    ui.add(
        Button::new(text)
            .selected(selected)
            .with_class(theme::CENTRED)
            .min_size(vec2(theme::CONTROL, theme::CONTROL)),
    )
}

pub const VALUE_BOX: f32 = 44.0;

pub fn labelled_slider(ui: &mut Ui, label: &str, slider: egui::Slider<'_>) -> Response {
    let in_a_row = ui.layout().main_dir() == egui::Direction::LeftToRight
        && ui.available_size_before_wrap().x < ui.max_rect().width() - 0.5;
    if in_a_row {
        ui.add_space(space::SM);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        ui.spacing_mut().interact_size.x = VALUE_BOX;
        let name = bar_label(ui, label);
        ui.add(slider).labelled_by(name.id)
    })
    .inner
}

pub fn percent(slider: egui::Slider<'_>) -> egui::Slider<'_> {
    slider
        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
        .custom_parser(|text| {
            let trimmed = text.trim();
            let shown = trimmed.trim_end_matches('%').trim().parse::<f64>().ok()?;
            Some(match trimmed.ends_with('%') || shown > 1.0 {
                true => shown / 100.0,
                false => shown,
            })
        })
}

pub trait PenScroll {
    fn pen_scroll(self, ctx: &egui::Context) -> Self;
}

impl PenScroll for egui::ScrollArea {
    fn pen_scroll(self, ctx: &egui::Context) -> Self {
        match crate::pen::in_use(ctx) {
            true => self.scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Always,
                ..egui::scroll_area::ScrollSource::default()
            }),
            false => self,
        }
    }
}
