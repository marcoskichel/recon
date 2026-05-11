//! Small one-line overlays drawn on top of cards (slot label `[N]`).

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    Frame,
};

/// Inputs to [`render_agent_label`].
pub(super) struct AgentLabelInputs {
    /// Card area to draw on top of.
    pub area: Rect,
    /// 1-based slot number to render inside `[N]`.
    pub index: usize,
    /// Foreground color of the label.
    pub color: Color,
    /// Cell offset from `area.x` (typically `0` for full cards, `1` to
    /// align with the inner edge of a rounded border).
    pub x_offset: u16,
}

/// Render the `[index]` overlay on the top edge of `inputs.area`.
pub(super) fn render_agent_label(frame: &mut Frame, inputs: &AgentLabelInputs) {
    let &AgentLabelInputs { area, index, color, x_offset } = inputs;

    let label = format!("[{index}]");
    let label_chars = label.chars().count();
    let label_chars_u16 = u16::try_from(label_chars).unwrap_or(u16::MAX);
    let label_w = label_chars_u16.min(area.width.saturating_sub(x_offset));
    if label_w == 0 {
        return;
    }
    let rect = Rect { x: area.x.saturating_add(x_offset), y: area.y, width: label_w, height: 1 };
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(label).style(style), rect);
}
