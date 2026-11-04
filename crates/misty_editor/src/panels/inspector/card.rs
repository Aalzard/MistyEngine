//! Inspector building blocks: the component card (rounded panel with a header bar and a
//! collapsible body), the aligned label / value grid, and bold sub-headers.
//!
//! Every card body sets one value column (see [`crate::widgets::set_value_column`]): labels
//! sit left of it in regular weight and values fill the width to its right, so rows of every
//! component (and nested grids inside them) line up.

use egui::collapsing_header::{CollapsingState, paint_default_icon};
use egui::{CornerRadius, Frame, Grid, Id, Margin, Response, Ui};

use crate::theme;
use crate::widgets::{LABEL_GAP, label_width_or, set_value_column};

/// Height of one property row.
pub const ROW_HEIGHT: f32 = 22.0;
const RADIUS: u8 = 6;
/// Vertical gap between cards.
const CARD_GAP: f32 = 5.0;

/// A component card. `header` draws the controls after the fold arrow (checkbox, icon, title,
/// right-aligned buttons) and returns the title, which also folds the card when clicked.
/// Both closures get `cx` (usually the editor state). Returns the body's value when open.
pub fn card<T, R>(
    ui: &mut Ui,
    id: Id,
    cx: &mut T,
    header: impl FnOnce(&mut Ui, &mut T) -> Response,
    body: impl FnOnce(&mut Ui, &mut T) -> R,
) -> Option<R> {
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, true);
    let r = Frame::new()
        .fill(theme::CARD_BG)
        .corner_radius(RADIUS)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let open = state.is_open();
            let header_radius = if open {
                CornerRadius {
                    nw: RADIUS,
                    ne: RADIUS,
                    sw: 0,
                    se: 0,
                }
            } else {
                CornerRadius::same(RADIUS)
            };
            let bar = Frame::new()
                .fill(theme::CARD_HEADER)
                .corner_radius(header_radius)
                .inner_margin(Margin {
                    left: 2,
                    right: 6,
                    top: 3,
                    bottom: 3,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.set_min_height(20.0);
                        state.show_toggle_button(ui, paint_default_icon);
                        header(ui, cx)
                    })
                    .inner
                });
            if bar.inner.clicked() {
                state.toggle(ui);
            }
            state
                .show_body_unindented(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin {
                            left: 10,
                            right: 8,
                            top: 6,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            begin_body(ui);
                            body(ui, cx)
                        })
                        .inner
                })
                .map(|r| r.inner)
        });
    ui.add_space(CARD_GAP);
    r.inner
}

/// Set the value column for a panel body starting at the cursor: about 40 % of the width.
pub fn begin_body(ui: &mut Ui) {
    let left = ui.cursor().left();
    let label = (ui.available_width() * 0.4).clamp(90.0, 170.0);
    set_value_column(ui, left + label + LABEL_GAP);
}

/// Two-column property grid: regular-weight labels on the left (truncated to the label
/// column), values filling the rest of the width.
pub fn grid<R>(ui: &mut Ui, id: impl egui::AsIdSalt, body: impl FnOnce(&mut Ui) -> R) -> R {
    PropertyGrid::new(id).show(ui, body).inner
}

/// [`grid`] as a drop-in for `egui::Grid::new(id).num_columns(2)…show(ui, …)`.
pub struct PropertyGrid {
    id: Id,
}

impl PropertyGrid {
    pub fn new(id: impl egui::AsId) -> Self {
        Self { id: Id::new(id) }
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
        let label_w = label_width_or(ui, 110.0);
        let value_w = (ui.available_width() - label_w - LABEL_GAP).max(60.0);
        ui.scope(|ui| {
            fit_widgets(ui, value_w);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            Grid::new(self.id)
                .num_columns(2)
                .spacing([LABEL_GAP, 4.0])
                .min_col_width(label_w)
                .min_row_height(ROW_HEIGHT)
                .show(ui, body)
                .inner
        })
    }
}

/// Size the default-width widgets (combo boxes, text fields, sliders, colour swatches) to fill
/// a value cell `value_w` wide. Number fields keep a compact minimum width.
pub fn fit_widgets(ui: &mut Ui, value_w: f32) {
    let s = ui.spacing_mut();
    let number_w = (value_w * 0.3).clamp(48.0, 76.0);
    s.combo_width = value_w;
    s.text_edit_width = value_w;
    s.interact_size.x = number_w;
    s.slider_width = (value_w - number_w - s.item_spacing.x).max(40.0);
}

/// Bold sub-section header ("Shadow Settings") between groups of rows, outside a grid.
pub fn sub_header(ui: &mut Ui, title: &str) -> Response {
    ui.add_space(6.0);
    let text = theme::bold(ui.ctx(), title);
    let r = ui.label(text);
    ui.add_space(1.0);
    r
}

/// Bold sub-section header as its own row inside a [`grid`].
pub fn grid_sub_header(ui: &mut Ui, title: &str) {
    let text = theme::bold(ui.ctx(), title);
    ui.label(text);
    ui.end_row();
}

/// Draw `add` with interact-sized widgets (colour swatches) filling the remaining width.
pub fn wide<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.x = ui.available_width().max(40.0);
        add(ui)
    })
    .inner
}
