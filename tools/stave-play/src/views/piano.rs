//! On-screen piano and computer-keyboard mapping: a compact strip (as
//! many octaves as fit, the computer keys' reach marked along its top),
//! the way a DAW docks its keyboard.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

/// The narrowest a white key gets before the strip drops an octave.
const MIN_WHITE: f32 = 22.0;
/// The highest semitone the computer keys reach above the octave root.
const KEY_SPAN: i16 = 16;

/// Computer-keyboard semitone mapping. (egui::Key, semitone offset from octave root.)
pub const KEY_TO_SEMITONE: &[(egui::Key, i16)] = &[
    (egui::Key::A, 0),
    (egui::Key::W, 1),
    (egui::Key::S, 2),
    (egui::Key::E, 3),
    (egui::Key::D, 4),
    (egui::Key::F, 5),
    (egui::Key::T, 6),
    (egui::Key::G, 7),
    (egui::Key::Y, 8),
    (egui::Key::H, 9),
    (egui::Key::U, 10),
    (egui::Key::J, 11),
    (egui::Key::K, 12),
    (egui::Key::O, 13),
    (egui::Key::L, 14),
    (egui::Key::P, 15),
    (egui::Key::Semicolon, KEY_SPAN),
];

pub fn midi_from_semi(octave: i32, semi: i16) -> i16 {
    (octave as i16 + 1) * 12 + semi
}

/// Notes held on the computer keyboard. None while Cmd, Ctrl or Alt is
/// down: those chords are shortcuts (Cmd+J must not also play a J).
pub fn keyboard_held(ctx: &egui::Context, octave: i32) -> HashSet<i16> {
    let mut held = HashSet::new();
    ctx.input(|i| {
        if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
            return;
        }
        for (key, semi) in KEY_TO_SEMITONE.iter() {
            if i.key_down(*key) {
                held.insert(midi_from_semi(octave, *semi));
            }
        }
    });
    held
}

pub fn note_name(midi: i16) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let n = midi.rem_euclid(12) as usize;
    let oct = midi / 12 - 1;
    format!("{}{}", NAMES[n], oct)
}

/// The octaves a strip `width` wide shows around `octave` (the computer
/// keys' root): as many as fit at [`MIN_WHITE`] per white key (two to
/// seven), starting an octave below the keys when there is room.
pub fn visible_octaves(width: f32, octave: i32) -> (i32, i32) {
    let fit = (width / (7.0 * MIN_WHITE)).floor() as i32;
    let count = fit.clamp(2, 7);
    let first = (octave - i32::from(count > 3)).clamp(0, 9 - count);
    (first, count)
}

/// (is black, white keys to its left) per semitone.
const SEMI_LAYOUT: [(bool, i32); 12] = [
    (false, 0),
    (true, 0),
    (false, 1),
    (true, 1),
    (false, 2),
    (false, 3),
    (true, 3),
    (false, 4),
    (true, 4),
    (false, 5),
    (true, 5),
    (false, 6),
];

/// Draw the piano: a strip `height` tall across the available width,
/// the computer keys' reach marked along its top in `accent`. Returns
/// the mouse-driven note (if the mouse is held on a key) for the caller
/// to fold into its held-set.
pub fn draw(
    ui: &mut egui::Ui,
    octave: i32,
    held_notes: &HashSet<i16>,
    height: f32,
    accent: Color32,
) -> Option<i16> {
    let available = ui.available_rect_before_wrap();
    // A fixed height: clamping to the space left would let a panel
    // that sizes to its content shrink the strip a little every frame.
    let height = height.max(24.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(available.width(), height),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let (first, count) = visible_octaves(rect.width(), octave);
    let mark = 3.0;
    let keys = Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + mark + 1.0), rect.max);
    let total_white = 7 * count;
    let white_w = keys.width() / total_white as f32;
    let black_w = white_w * 0.6;
    let black_h = keys.height() * 0.6;

    let mut white_rects: Vec<(Rect, i16)> = Vec::with_capacity(total_white as usize);
    let mut black_rects: Vec<(Rect, i16)> = Vec::with_capacity(5 * count as usize);
    for oct in 0..count {
        for (semi, &(is_black, white_idx)) in SEMI_LAYOUT.iter().enumerate() {
            let pitch = midi_from_semi(first + oct, semi as i16);
            let x = keys.min.x + (oct * 7 + white_idx) as f32 * white_w;
            if is_black {
                let r = Rect::from_min_size(
                    Pos2::new(x + white_w - black_w * 0.5, keys.min.y),
                    Vec2::new(black_w, black_h),
                );
                black_rects.push((r, pitch));
            } else {
                let r = Rect::from_min_size(Pos2::new(x, keys.min.y), Vec2::new(white_w, keys.height()));
                white_rects.push((r, pitch));
            }
        }
    }

    let mouse_note = if response.is_pointer_button_down_on() {
        response.interact_pointer_pos().and_then(|pos| {
            black_rects
                .iter()
                .chain(&white_rects)
                .find(|(r, _)| r.contains(pos))
                .map(|(_, pitch)| *pitch)
        })
    } else {
        None
    };

    // The computer keys' reach, as a bar over the keys it plays.
    let (lo, hi) = (midi_from_semi(octave, 0), midi_from_semi(octave, KEY_SPAN));
    let in_reach = || {
        white_rects
            .iter()
            .chain(&black_rects)
            .filter(move |(_, p)| (lo..=hi).contains(p))
            .map(|(r, _)| *r)
    };
    let left = in_reach().map(|r| r.min.x).reduce(f32::min);
    let right = in_reach().map(|r| r.max.x).reduce(f32::max);
    if let (Some(l), Some(r)) = (left, right) {
        let bar = Rect::from_min_max(Pos2::new(l, rect.min.y), Pos2::new(r, rect.min.y + mark));
        painter.rect_filled(bar, 1.0, accent);
    }

    let lit = |pitch: &i16| held_notes.contains(pitch) || mouse_note == Some(*pitch);
    for (r, pitch) in &white_rects {
        let fill = if lit(pitch) { accent } else { Color32::from_rgb(236, 232, 224) };
        painter.rect_filled(r.shrink2(Vec2::new(0.5, 0.0)), 2.0, fill);
        if (*pitch % 12) == 0 && white_w >= 16.0 {
            painter.text(
                Pos2::new(r.center().x, r.max.y - 8.0),
                Align2::CENTER_CENTER,
                format!("C{}", *pitch / 12 - 1),
                FontId::proportional(9.0),
                Color32::from_gray(120),
            );
        }
    }
    for (r, pitch) in &black_rects {
        let fill = if lit(pitch) {
            accent.gamma_multiply(0.75)
        } else {
            Color32::from_rgb(28, 29, 31)
        };
        painter.rect_filled(*r, 2.0, fill);
        painter.rect_stroke(*r, 2.0, Stroke::new(1.0, Color32::from_gray(50)));
    }

    mouse_note
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_shows_as_many_octaves_as_fit_around_the_keys() {
        // Narrow: two octaves, starting at the keys' root.
        assert_eq!(visible_octaves(300.0, 4), (4, 2));
        // Wide: an octave of context below the keys.
        assert_eq!(visible_octaves(1000.0, 4), (3, 6));
        // Never more than seven, never past the top of MIDI's range.
        assert_eq!(visible_octaves(5000.0, 8), (2, 7));
        assert_eq!(visible_octaves(5000.0, 0), (0, 7));
    }

    #[test]
    fn shortcut_chords_play_no_notes() {
        let ctx = egui::Context::default();
        let held = |modifiers: egui::Modifiers| {
            let input = egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key: egui::Key::J,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let mut out = HashSet::new();
            let _ = ctx.run(input, |c| out = keyboard_held(c, 4));
            out
        };
        assert_eq!(held(egui::Modifiers::NONE), HashSet::from([midi_from_semi(4, 11)]));
        // Release, then the same key under Cmd: a shortcut, not a note.
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::J,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| {},
        );
        assert!(held(egui::Modifiers::COMMAND).is_empty());
    }
}
