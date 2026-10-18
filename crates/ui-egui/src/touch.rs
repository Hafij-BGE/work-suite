//! Touch gestures for phones and tablets (the Android build).
//!
//! egui turns the first finger into a mouse: a press where it lands and a held button while it
//! moves, so dragging a finger over the page selected text instead of scrolling it. This rewrites
//! a frame's input before egui sees it, so the editor behaves like a touch app:
//! - **drag** one finger: scroll (touchpad-style pixel deltas, so the page follows the finger and
//!   coasts after a flick, see `scroll.rs`); the press is held back until the finger moves or
//!   lifts, so scrolling never moves the caret;
//! - **tap**: a click (places the caret, presses a button); **double-tap** selects a word;
//! - **double-tap and drag**: the second press goes through at once, so the drag selects word by
//!   word (or moves a picture, a slider, a ruler marker) as a mouse drag does;
//! - **long press**: a right-click, which opens the page's menu (Cut, Copy, Paste…);
//! - **pinch** two fingers: zoom (the canvas reads egui's multi-touch zoom); the pointer is ignored
//!   until every finger has lifted.
//!
//! Input that doesn't come from a touch (a mouse or a stylus egui reports as a mouse) is passed on
//! untouched.

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchId, TouchPhase, Vec2};

/// A finger that moves further than this (points) from where it landed scrolls.
const SLOP: f32 = 10.0;
/// A finger held this long (seconds) without moving is a long press.
const LONG_PRESS: f64 = 0.5;
/// A press this soon after a tap (seconds), and this close to it (points), is a double-tap.
const DOUBLE_TAP_DELAY: f64 = 0.35;
const DOUBLE_TAP_DIST: f32 = 24.0;

#[derive(Clone, Debug, Default, PartialEq)]
enum State {
    /// No finger down (or input from a mouse).
    #[default]
    Idle,
    /// A finger is down; its press is held back until it is clear what the finger does.
    Pending { start: Pos2, at: f64, modifiers: Modifiers },
    /// The finger is scrolling.
    Panning { last: Pos2 },
    /// The press went through (a double-tap): egui sees a held mouse button.
    Forwarded,
    /// A pinch, or a long press already answered: the pointer is ignored until the fingers lift.
    Ignoring,
}

/// The gesture state across frames; [`TouchGestures::process`] runs on every frame's input.
#[derive(Clone, Debug, Default)]
pub struct TouchGestures {
    state: State,
    /// The fingers on the screen.
    fingers: Vec<TouchId>,
    /// When and where the last tap was, for double-taps.
    last_tap: Option<(f64, Pos2)>,
    /// The input time of the last frame (`RawInput::time` can be missing).
    now: f64,
    /// Where the pointer last was.
    pointer: Pos2,
}

impl TouchGestures {
    /// Whether a finger is on the screen.
    pub fn touching(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// Rewrite this frame's touch input in place. Returns how many seconds until a frame is needed
    /// to recognise a long press (the finger may lie still, sending nothing), if one is.
    pub fn process(&mut self, raw: &mut RawInput) -> Option<f64> {
        if let Some(t) = raw.time.filter(|t| t.is_finite()) {
            self.now = t;
        }
        let now = self.now;
        let events = std::mem::take(&mut raw.events);
        let mut out = Vec::with_capacity(events.len() + 2);
        for event in events {
            match event {
                Event::Touch { id, phase, .. } => {
                    match phase {
                        TouchPhase::Start => {
                            if !self.fingers.contains(&id) {
                                self.fingers.push(id);
                            }
                            if self.fingers.len() > 1 {
                                self.start_pinch(&mut out);
                            }
                        }
                        TouchPhase::End | TouchPhase::Cancel => self.fingers.retain(|f| *f != id),
                        TouchPhase::Move => {}
                    }
                    // egui reads the fingers themselves for pinch zoom.
                    out.push(event);
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers } if self.touching() => match self.state {
                    State::Idle if self.double_tap(now, pos) => {
                        self.state = State::Forwarded;
                        out.push(event);
                    }
                    State::Idle => self.state = State::Pending { start: pos, at: now, modifiers },
                    // A press egui sends again for the same finger, or after a pinch.
                    _ => {}
                },
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers } => match self.state {
                    State::Pending { start, modifiers: held, .. } => {
                        // A tap: the press and the release together, where the finger landed.
                        out.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: held });
                        out.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: false, modifiers });
                        self.last_tap = Some((now, start));
                        self.state = State::Idle;
                    }
                    State::Panning { .. } => {
                        out.push(wheel(Vec2::ZERO, TouchPhase::End));
                        self.state = State::Idle;
                    }
                    State::Forwarded => {
                        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
                        self.last_tap = None;
                        self.state = State::Idle;
                    }
                    State::Ignoring => {}
                    State::Idle => out.push(event),
                },
                Event::PointerMoved(p) => {
                    self.pointer = p;
                    match self.state {
                        State::Pending { start, .. } if (p - start).length() > SLOP => {
                            out.push(Event::PointerMoved(p));
                            out.push(wheel(p - start, TouchPhase::Start));
                            self.state = State::Panning { last: p };
                            continue;
                        }
                        State::Panning { last } => {
                            out.push(Event::PointerMoved(p));
                            out.push(wheel(p - last, TouchPhase::Move));
                            self.state = State::Panning { last: p };
                            continue;
                        }
                        _ => {}
                    }
                    out.push(event);
                }
                Event::PointerGone => {
                    match self.state {
                        // A cancelled touch: nothing is pressed yet.
                        State::Pending { .. } | State::Ignoring => self.state = State::Idle,
                        State::Panning { .. } => {
                            out.push(wheel(Vec2::ZERO, TouchPhase::End));
                            self.state = State::Idle;
                        }
                        State::Forwarded | State::Idle => {}
                    }
                    out.push(event);
                }
                // While a touch is held back or scrolling, other buttons wait too.
                Event::PointerButton { .. } if matches!(self.state, State::Pending { .. } | State::Panning { .. } | State::Ignoring) => {}
                _ => out.push(event),
            }
        }
        // A finger lying still long enough: a right-click where it is (the page's menu).
        let mut wait = None;
        if let State::Pending { start, at, modifiers } = self.state {
            let held = now - at;
            if held >= LONG_PRESS {
                out.push(Event::PointerMoved(start));
                out.push(Event::PointerButton { pos: start, button: PointerButton::Secondary, pressed: true, modifiers });
                out.push(Event::PointerButton { pos: start, button: PointerButton::Secondary, pressed: false, modifiers });
                self.last_tap = None;
                self.state = State::Ignoring;
            } else {
                wait = Some((LONG_PRESS - held).max(0.0));
            }
        }
        if self.state == State::Ignoring && self.fingers.is_empty() {
            self.state = State::Idle;
        }
        raw.events = out;
        wait
    }

    /// A second finger landed: whatever the first one was doing ends, and the pinch begins.
    fn start_pinch(&mut self, out: &mut Vec<Event>) {
        match self.state {
            State::Panning { .. } => out.push(wheel(Vec2::ZERO, TouchPhase::End)),
            // Let go of the held button where the pointer is.
            State::Forwarded => {
                out.push(Event::PointerButton { pos: self.pointer, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE })
            }
            _ => {}
        }
        self.last_tap = None;
        self.state = State::Ignoring;
    }

    fn double_tap(&self, now: f64, pos: Pos2) -> bool {
        self.last_tap.is_some_and(|(at, p)| now - at <= DOUBLE_TAP_DELAY && (pos - p).length() <= DOUBLE_TAP_DIST)
    }
}

/// A touchpad-style scroll by `delta` points (positive = content moves down/right, as under a
/// finger moving down/right).
fn wheel(delta: Vec2, phase: TouchPhase) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{TouchDeviceId, pos2, vec2};

    fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos, force: None }
    }

    fn press(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// What egui-winit sends for a finger landing, moving and lifting.
    fn down(pos: Pos2) -> Vec<Event> {
        vec![touch(1, TouchPhase::Start, pos), Event::PointerMoved(pos), press(pos, true)]
    }
    fn moved(pos: Pos2) -> Vec<Event> {
        vec![touch(1, TouchPhase::Move, pos), Event::PointerMoved(pos)]
    }
    fn up(pos: Pos2) -> Vec<Event> {
        vec![touch(1, TouchPhase::End, pos), press(pos, false), Event::PointerGone]
    }

    fn frame(g: &mut TouchGestures, time: f64, events: Vec<Event>) -> (Vec<Event>, Option<f64>) {
        let mut raw = RawInput { time: Some(time), events, ..Default::default() };
        let wait = g.process(&mut raw);
        (raw.events, wait)
    }

    fn buttons(events: &[Event]) -> Vec<(PointerButton, bool)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::PointerButton { button, pressed, .. } => Some((*button, *pressed)),
                _ => None,
            })
            .collect()
    }

    fn scrolled(events: &[Event]) -> Vec2 {
        events
            .iter()
            .map(|e| match e {
                Event::MouseWheel { unit: MouseWheelUnit::Point, delta, .. } => *delta,
                _ => Vec2::ZERO,
            })
            .fold(Vec2::ZERO, |a, b| a + b)
    }

    #[test]
    fn a_tap_is_a_click_where_the_finger_landed() {
        let mut g = TouchGestures::default();
        let (out, wait) = frame(&mut g, 0.0, down(pos2(50.0, 50.0)));
        assert!(buttons(&out).is_empty(), "the press is held back: {out:?}");
        assert!(wait.is_some_and(|w| w > 0.0), "a frame is asked for, for a long press");
        let (out, _) = frame(&mut g, 0.1, up(pos2(52.0, 51.0)));
        assert_eq!(buttons(&out), [(PointerButton::Primary, true), (PointerButton::Primary, false)]);
        assert!(out.iter().any(|e| matches!(e, Event::PointerButton { pos, pressed: true, .. } if *pos == pos2(50.0, 50.0))));
        assert!(!g.touching());
    }

    #[test]
    fn a_drag_scrolls_and_never_presses() {
        let mut g = TouchGestures::default();
        let mut all = frame(&mut g, 0.0, down(pos2(100.0, 300.0))).0;
        for (i, y) in [295.0, 280.0, 250.0, 200.0].into_iter().enumerate() {
            all.extend(frame(&mut g, 0.02 * (i + 1) as f64, moved(pos2(100.0, y))).0);
        }
        all.extend(frame(&mut g, 0.2, up(pos2(100.0, 200.0))).0);
        assert!(buttons(&all).is_empty(), "no click, no selection: {all:?}");
        assert_eq!(scrolled(&all), vec2(0.0, -100.0), "the page follows the finger");
        assert!(all.iter().any(|e| matches!(e, Event::MouseWheel { phase: TouchPhase::End, .. })), "the lift ends the scroll (and lets it coast)");
        // Moves within the slop don't scroll yet.
        let (out, _) = frame(&mut g, 1.0, down(pos2(10.0, 10.0)));
        assert_eq!(scrolled(&out), Vec2::ZERO);
        let (out, _) = frame(&mut g, 1.02, moved(pos2(14.0, 15.0)));
        assert_eq!(scrolled(&out), Vec2::ZERO);
    }

    #[test]
    fn a_long_press_is_a_right_click_and_the_rest_is_ignored() {
        let mut g = TouchGestures::default();
        frame(&mut g, 0.0, down(pos2(40.0, 40.0)));
        let (out, wait) = frame(&mut g, 0.3, vec![]);
        assert!(buttons(&out).is_empty());
        assert!(wait.is_some_and(|w| (w - 0.2).abs() < 1e-9), "{wait:?}");
        let (out, wait) = frame(&mut g, 0.55, vec![]);
        assert_eq!(buttons(&out), [(PointerButton::Secondary, true), (PointerButton::Secondary, false)]);
        assert_eq!(wait, None);
        let mut rest = frame(&mut g, 0.7, moved(pos2(90.0, 90.0))).0;
        rest.extend(frame(&mut g, 0.8, up(pos2(90.0, 90.0))).0);
        assert!(buttons(&rest).is_empty() && scrolled(&rest) == Vec2::ZERO, "{rest:?}");
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn double_tap_and_drag_selects() {
        let mut g = TouchGestures::default();
        frame(&mut g, 0.0, down(pos2(60.0, 60.0)));
        frame(&mut g, 0.08, up(pos2(60.0, 60.0)));
        let (out, _) = frame(&mut g, 0.2, down(pos2(62.0, 61.0)));
        assert_eq!(buttons(&out), [(PointerButton::Primary, true)], "the second press goes through at once");
        let (out, _) = frame(&mut g, 0.3, moved(pos2(160.0, 61.0)));
        assert_eq!(scrolled(&out), Vec2::ZERO, "the drag selects instead of scrolling");
        assert!(out.contains(&Event::PointerMoved(pos2(160.0, 61.0))));
        let (out, _) = frame(&mut g, 0.4, up(pos2(160.0, 61.0)));
        assert_eq!(buttons(&out), [(PointerButton::Primary, false)]);
        // A third tap later is a plain tap again.
        let (out, _) = frame(&mut g, 2.0, down(pos2(62.0, 61.0)));
        assert!(buttons(&out).is_empty());
    }

    #[test]
    fn a_pinch_drops_the_pointer_until_every_finger_lifts() {
        let mut g = TouchGestures::default();
        frame(&mut g, 0.0, down(pos2(100.0, 100.0)));
        let (out, _) = frame(&mut g, 0.05, vec![touch(2, TouchPhase::Start, pos2(200.0, 200.0))]);
        assert!(out.iter().any(|e| matches!(e, Event::Touch { id: TouchId(2), .. })), "egui still sees the fingers");
        let mut rest = frame(&mut g, 0.1, moved(pos2(80.0, 80.0))).0;
        rest.extend(frame(&mut g, 0.2, vec![touch(2, TouchPhase::End, pos2(220.0, 220.0))]).0);
        rest.extend(frame(&mut g, 0.9, vec![]).0);
        rest.extend(frame(&mut g, 1.0, up(pos2(80.0, 80.0))).0);
        assert!(buttons(&rest).is_empty() && scrolled(&rest) == Vec2::ZERO, "{rest:?}");
        assert!(!g.touching());
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn mouse_input_passes_through() {
        let mut g = TouchGestures::default();
        let events = vec![
            Event::PointerMoved(pos2(5.0, 5.0)),
            press(pos2(5.0, 5.0), true),
            Event::PointerMoved(pos2(50.0, 5.0)),
            press(pos2(50.0, 5.0), false),
        ];
        let (out, wait) = frame(&mut g, 0.0, events.clone());
        assert_eq!(out, events);
        assert_eq!(wait, None);
    }

    #[test]
    fn a_cancelled_touch_leaves_nothing_pressed() {
        let mut g = TouchGestures::default();
        frame(&mut g, 0.0, down(pos2(10.0, 10.0)));
        let (out, _) = frame(&mut g, 0.1, vec![touch(1, TouchPhase::Cancel, pos2(10.0, 10.0)), Event::PointerGone]);
        assert!(buttons(&out).is_empty());
        assert_eq!(g.state, State::Idle);
        assert!(!g.touching());
    }
}
