//! Shared sprite primitives, palette indirection, and layout constants.
//!
//! All numbers used by the rest of the renderer pipeline live here so that
//! sprite data, sprite renderers, and view code can agree on a single set
//! of dimensions without duplicating literals.

/// Pixel columns in a sprite.
pub(super) const SPRITE_W: usize = 10;
/// Pixel rows in a sprite.
pub(super) const SPRITE_H: usize = 10;

/// Terminal lines required to draw a full half-block sprite (two pixels per cell).
pub(super) const SPRITE_RENDER_H: u16 = 5;

/// Card width used for a non-compact agent character (sprite + label padding).
pub(super) const CHAR_WIDTH: u16 = 40;
/// Number of lines used to wrap an agent label across.
pub(super) const NAME_LINES: u16 = 3;
/// Agent label rows: `NAME_LINES` lines for name, plus branch and context bar.
pub(super) const CHAR_LABEL_LINES: u16 = NAME_LINES + 2;
/// Total terminal rows occupied by a non-compact agent card.
pub(super) const CHAR_HEIGHT: u16 = SPRITE_RENDER_H + CHAR_LABEL_LINES;

/// Compact horizontal card width: sprite left, info right, rounded border.
pub(super) const COMPACT_CARD_WIDTH: u16 = 46;
/// Compact card height in terminal rows.
pub(super) const COMPACT_CARD_HEIGHT: u16 = 9;
/// Sprite area within the compact card (sprite + 1-col gutter on each side).
pub(super) const COMPACT_SPRITE_COLS: u16 = 12;

/// Sextant-mini sprite columns (each cell encodes two pixel columns).
pub(super) const MINI_SPRITE_W: u16 = 5;
/// Sextant-mini sprite rows (each cell encodes three pixel rows).
pub(super) const MINI_SPRITE_H: u16 = 4;
/// Dock card width: mini sprite (5) + horizontal padding (2) + border (2).
pub(super) const DOCK_CARD_W: u16 = MINI_SPRITE_W + 4;
/// Dock card height: borders (2) + sprite (4) + thin bar (1).
pub(super) const DOCK_CARD_H: u16 = MINI_SPRITE_H + 3;

/// 10x10 pixel sprite. Each cell is a palette index (0 = transparent).
pub(super) type Sprite = [[u8; SPRITE_W]; SPRITE_H];

/// Palette: ordered list of `(r, g, b)` triples. Index 0 is reserved for
/// the transparent slot and is never sampled by the renderer.
pub(super) type Palette = &'static [(u8, u8, u8)];

/// Number of species (pup variants) supported.
pub const SPECIES_COUNT: usize = 10;

/// Display names indexed by species id, in palette order.
pub const SPECIES_NAMES: [&str; SPECIES_COUNT] =
    ["Floppy", "Fox", "Blob", "Bolt", "Turtle", "Wisp", "Penguin", "Sprout", "Beetle", "Shadow"];
