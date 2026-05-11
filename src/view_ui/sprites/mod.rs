//! Sprite resolution and pixel-to-glyph rendering.
//!
//! Two renderers are provided:
//!
//! * [`render_sprite_lines`] — half-block renderer: each terminal cell encodes
//!   two pixel rows (▀/▄). Used for full-size sprites in agent cards and
//!   loading/empty placeholders.
//! * [`render_sprite_compact`] — sextant renderer: each terminal cell encodes
//!   a 2x3 pixel block. Used for the dock view's mini sprites.

mod idle_data;
mod input_data;
mod new_data;
mod working_data;

use idle_data::IDLE_SPRITES;
use input_data::INPUT_SPRITES;
use new_data::NEW_SPRITES;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use working_data::WORKING_SPRITES;

use super::{
    palettes::SPECIES_PALETTES,
    types::{Palette, Sprite, SPECIES_COUNT, SPRITE_H, SPRITE_W},
};
use crate::session::SessionStatus;

/// Number of animation frames in [`WORKING_SPRITES`] and [`INPUT_SPRITES`].
const ANIM_FRAMES: usize = 3;

/// Resolve a `(sprite, palette)` pair for the given session state.
///
/// `species` is taken modulo [`SPECIES_COUNT`] to keep the indexing total.
pub(super) fn sprite_data(
    status: &SessionStatus,
    frame: usize,
    species: usize,
) -> (&'static Sprite, Palette) {
    let species_id = species % SPECIES_COUNT;
    let frame_id = frame % ANIM_FRAMES;
    let palette = SPECIES_PALETTES.get(species_id).copied().unwrap_or(SPECIES_PALETTES[0]);
    let sprite: &'static Sprite = match *status {
        SessionStatus::New => NEW_SPRITES
            .get(species_id)
            .and_then(|frames| frames.first())
            .unwrap_or(&NEW_SPRITES[0][0]),
        SessionStatus::Working => WORKING_SPRITES
            .get(species_id)
            .and_then(|frames| frames.get(frame_id))
            .unwrap_or(&WORKING_SPRITES[0][0]),
        SessionStatus::Idle => IDLE_SPRITES
            .get(species_id)
            .and_then(|frames| frames.first())
            .unwrap_or(&IDLE_SPRITES[0][0]),
        SessionStatus::Input => INPUT_SPRITES
            .get(species_id)
            .and_then(|frames| frames.get(frame_id))
            .unwrap_or(&INPUT_SPRITES[0][0]),
    };
    (sprite, palette)
}

/// Look up an `(r, g, b)` tuple in `palette`, falling back to `(0, 0, 0)` on
/// out-of-range indices. Keeps the renderer total without panics.
fn palette_rgb(palette: Palette, index: u8) -> (u8, u8, u8) {
    palette.get(usize::from(index)).copied().unwrap_or((0, 0, 0))
}

/// Look up a pixel in `sprite` at `(line, column)`, returning `0` if out of bounds.
fn sprite_pixel(sprite: &Sprite, line: usize, column: usize) -> u8 {
    sprite.get(line).and_then(|cells| cells.get(column)).copied().unwrap_or(0)
}

/// Render a sprite as ▀/▄ half-block lines (each line covers 2 pixel rows).
pub(super) fn render_sprite_lines(sprite: &Sprite, palette: Palette) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line_idx = 0usize;
    while line_idx < SPRITE_H {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for col_idx in 0..SPRITE_W {
            let upper = sprite_pixel(sprite, line_idx, col_idx);
            let lower = sprite_pixel(sprite, line_idx.saturating_add(1), col_idx);
            spans.push(half_block_span(palette, upper, lower));
        }
        lines.push(Line::from(spans));
        line_idx = line_idx.saturating_add(2);
    }
    lines
}

/// Encode a pair of vertically adjacent pixels as a single styled span.
fn half_block_span(palette: Palette, upper: u8, lower: u8) -> Span<'static> {
    if upper == 0 && lower == 0 {
        Span::raw(" ")
    } else if upper == 0 {
        let (channel_r, channel_g, channel_b) = palette_rgb(palette, lower);
        Span::styled("\u{2584}", Style::default().fg(Color::Rgb(channel_r, channel_g, channel_b)))
    } else if lower == 0 {
        let (channel_r, channel_g, channel_b) = palette_rgb(palette, upper);
        Span::styled("\u{2580}", Style::default().fg(Color::Rgb(channel_r, channel_g, channel_b)))
    } else {
        let (upper_r, upper_g, upper_b) = palette_rgb(palette, upper);
        let (lower_r, lower_g, lower_b) = palette_rgb(palette, lower);
        Span::styled(
            "\u{2580}",
            Style::default()
                .fg(Color::Rgb(upper_r, upper_g, upper_b))
                .bg(Color::Rgb(lower_r, lower_g, lower_b)),
        )
    }
}

/// Translate a 6-bit sextant pattern into the matching Unicode glyph.
///
/// Bit positions: TL=0, TR=1, ML=2, MR=3, BL=4, BR=5. The all-on pattern is
/// the full block (U+2588); the half-column patterns map to U+258C and U+2590.
fn sextant_glyph(bits: u8) -> String {
    match bits {
        0 => " ".to_string(),
        21 => "\u{258C}".to_string(),
        42 => "\u{2590}".to_string(),
        63 => "\u{2588}".to_string(),
        other => sextant_glyph_other(other),
    }
}

/// Look up the glyph for a sextant bit pattern that is not one of the
/// special-cased reserved/half/full slots.
fn sextant_glyph_other(bits: u8) -> String {
    let bits_u32 = u32::from(bits);
    let skip = u32::from(bits_u32 > 0)
        .saturating_add(u32::from(bits_u32 > 21))
        .saturating_add(u32::from(bits_u32 > 42));
    let offset = bits_u32.saturating_sub(skip);
    let base: u32 = 0x1FB00;
    base.checked_add(offset)
        .and_then(char::from_u32)
        .map_or_else(|| " ".to_string(), |character| character.to_string())
}

/// Render a sprite using sextant glyphs (2x3 pixels per cell).
///
/// Produces a `5 * 4` cell grid for the standard 10x10 sprite.
pub(super) fn render_sprite_compact(sprite: &Sprite, palette: Palette) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line_idx = 0usize;
    while line_idx < SPRITE_H {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut col_idx = 0usize;
        while col_idx < SPRITE_W {
            spans.push(sextant_cell_span(sprite, palette, line_idx, col_idx));
            col_idx = col_idx.saturating_add(2);
        }
        lines.push(Line::from(spans));
        line_idx = line_idx.saturating_add(3);
    }
    lines
}

/// Encode a 2x3 pixel block at `(line, column)` as a styled sextant span.
fn sextant_cell_span(
    sprite: &Sprite,
    palette: Palette,
    line: usize,
    column: usize,
) -> Span<'static> {
    let pixels: [u8; 6] = [
        sprite_pixel(sprite, line, column),
        sprite_pixel(sprite, line, column.saturating_add(1)),
        sprite_pixel(sprite, line.saturating_add(1), column),
        sprite_pixel(sprite, line.saturating_add(1), column.saturating_add(1)),
        sprite_pixel(sprite, line.saturating_add(2), column),
        sprite_pixel(sprite, line.saturating_add(2), column.saturating_add(1)),
    ];

    let (fg_idx, bg_idx) = dominant_colors(pixels);
    if fg_idx == 0 {
        return Span::raw(" ");
    }

    let bits = sextant_bits(pixels, fg_idx, bg_idx);
    let glyph = sextant_glyph(bits);
    let (fg_r, fg_g, fg_b) = palette_rgb(palette, fg_idx);
    let style = if bg_idx == 0 {
        Style::default().fg(Color::Rgb(fg_r, fg_g, fg_b))
    } else {
        let (bg_r, bg_g, bg_b) = palette_rgb(palette, bg_idx);
        Style::default().fg(Color::Rgb(fg_r, fg_g, fg_b)).bg(Color::Rgb(bg_r, bg_g, bg_b))
    };
    Span::styled(glyph, style)
}

/// Pick the two most frequent non-transparent colors in `pixels`.
///
/// Returns `(foreground, background)`. Either may be `0` to mean "absent":
/// when no non-zero pixels exist, both are zero; when only one color is
/// present, the background is zero.
fn dominant_colors(pixels: [u8; 6]) -> (u8, u8) {
    let mut freq: [(u8, u8); 12] = [(0, 0); 12];
    let mut filled: usize = 0;
    for &pixel in &pixels {
        if pixel == 0 {
            continue;
        }
        let already = freq
            .get_mut(..filled)
            .and_then(|slice| slice.iter_mut().find(|&&mut (color, _)| color == pixel));
        if let Some(entry) = already {
            entry.1 = entry.1.saturating_add(1);
        } else if let Some(slot) = freq.get_mut(filled) {
            *slot = (pixel, 1);
            filled = filled.saturating_add(1);
        } else {
            // Frequency table at capacity: ignore the additional color.
        }
    }
    if filled == 0 {
        return (0, 0);
    }
    if let Some(slice) = freq.get_mut(..filled) {
        slice.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    }
    let foreground = freq.first().map_or(0, |entry| entry.0);
    let background = if filled > 1 { freq.get(1).map_or(0, |entry| entry.0) } else { 0 };
    (foreground, background)
}

/// Pack a 2x3 pixel block into a 6-bit sextant pattern.
///
/// A bit is set when the pixel matches the foreground color or is a
/// non-background "speckle" (anything other than zero or `bg_idx`).
fn sextant_bits(pixels: [u8; 6], fg_idx: u8, bg_idx: u8) -> u8 {
    let bit_for =
        |pixel: u8| -> u8 { u8::from(pixel == fg_idx || (pixel != 0 && pixel != bg_idx)) };
    bit_for(pixels[0])
        | (bit_for(pixels[1]) << 1)
        | (bit_for(pixels[2]) << 2)
        | (bit_for(pixels[3]) << 3)
        | (bit_for(pixels[4]) << 4)
        | (bit_for(pixels[5]) << 5)
}
