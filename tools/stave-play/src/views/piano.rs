//! On-screen piano and computer-keyboard mapping. Extracted from the original
//! HostApp so the analysis tabs don't have to know about note handling.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

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
    (egui::Key::Semicolon, 16),
];

pub fn midi_from_semi(octave: i32, semi: i16) -> i16 {
    (octave as i16 + 1) * 12 + semi
}

pub fn keyboard_held(ctx: &egui::Context, octave: i32) -> HashSet<i16> {
    let mut held = HashSet::new();
    ctx.input(|i| {
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

/// Draw the piano. Returns the mouse-driven note (if the mouse is held on a
/// key) for the caller to fold into its held-set.
pub fn draw(ui: &mut egui::Ui, octave: i32, held_notes: &HashSet<i16>) -> Option<i16> {
    let available = ui.available_rect_before_wrap();
    let height = 180.0_f32.min(available.height());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(available.width(), height),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);

    let total_white = 14;
    let white_w = rect.width() / total_white as f32;
    let white_h = rect.height();
    let black_w = white_w * 0.6;
    let black_h = white_h * 0.62;

    const SEMI_LAYOUT: [(bool, i16); 12] = [
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

    let mut white_rects: Vec<(Rect, i16)> = Vec::with_capacity(14);
    let mut black_rects: Vec<(Rect, i16)> = Vec::with_capacity(10);
    for oct in 0..2 {
        for semi in 0..12 {
            let (is_black, white_idx) = SEMI_LAYOUT[semi as usize];
            let pitch = midi_from_semi(octave, oct * 12 + semi);
            if is_black {
                let left =
                    rect.min.x + (oct * 7 + white_idx) as f32 * white_w + white_w - black_w * 0.5;
                let r =
                    Rect::from_min_size(Pos2::new(left, rect.min.y), Vec2::new(black_w, black_h));
                black_rects.push((r, pitch));
            } else {
                let left = rect.min.x + (oct * 7 + white_idx) as f32 * white_w;
                let r =
                    Rect::from_min_size(Pos2::new(left, rect.min.y), Vec2::new(white_w, white_h));
                white_rects.push((r, pitch));
            }
        }
    }

    let mouse_note = if response.is_pointer_button_down_on() {
        response.interact_pointer_pos().and_then(|pos| {
            for (r, pitch) in &black_rects {
                if r.contains(pos) {
                    return Some(*pitch);
                }
            }
            for (r, pitch) in &white_rects {
                if r.contains(pos) {
                    return Some(*pitch);
                }
            }
            None
        })
    } else {
        None
    };

    for (r, pitch) in &white_rects {
        let active = held_notes.contains(pitch) || mouse_note == Some(*pitch);
        let fill = if active {
            Color32::from_rgb(190, 220, 255)
        } else {
            Color32::from_rgb(245, 245, 245)
        };
        painter.rect_filled(*r, 3.0, fill);
        painter.rect_stroke(*r, 3.0, Stroke::new(1.0, Color32::from_gray(110)));
        if (*pitch % 12) == 0 {
            painter.text(
                Pos2::new(r.center().x, r.max.y - 12.0),
                Align2::CENTER_CENTER,
                format!("C{}", *pitch / 12 - 1),
                FontId::proportional(11.0),
                Color32::from_gray(140),
            );
        }
    }
    for (r, pitch) in &black_rects {
        let active = held_notes.contains(pitch) || mouse_note == Some(*pitch);
        let fill = if active {
            Color32::from_rgb(80, 130, 200)
        } else {
            Color32::from_rgb(30, 30, 30)
        };
        painter.rect_filled(*r, 2.0, fill);
        painter.rect_stroke(*r, 2.0, Stroke::new(1.0, Color32::from_gray(60)));
    }

    mouse_note
}
