//! WorkSuite patch (see `PATCHED.md` at the crate root): a stylus, and a mouse, work as a mouse.
//!
//! Upstream winit 0.30 reports every pointer of an Android `MotionEvent` as a touch and drops
//! hover events, so a pen drags like a finger (an app that scrolls under a finger can't be
//! written with one). Here a pointer whose tool is a stylus, its eraser end or a mouse becomes
//! the cursor: hovering moves it, touching presses the left button (the right one while the pen's
//! side button is held, or for a mouse's secondary button), lifting releases it.
//!
//! Fingers that touch while the pen is on the screen, or near it (hovering, or lifted less than
//! [`PALM_GRACE`] ago), are dropped: that is the hand resting on the screen. A finger already down
//! when the pen came keeps working until it lifts.
//!
//! What the pen is doing (touching, hovering, its eraser end, its button, its pressure) is read
//! with [`crate::platform::android::stylus`].

use std::sync::Mutex;
use std::time::{Duration, Instant};

use android_activity::input::{MotionAction, MotionEvent, ToolType};

use crate::dpi::PhysicalPosition;
use crate::event::TouchPhase;

/// After the pen leaves the screen, a finger that lands is still taken for the palm this long.
pub const PALM_GRACE: Duration = Duration::from_millis(600);

/// What a stylus is doing, as of the last event from it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StylusState {
    /// The pen touches the screen.
    pub touching: bool,
    /// The pen is near the screen without touching it.
    pub hovering: bool,
    /// The pen is upside down: its eraser end is the one near the screen.
    pub eraser: bool,
    /// The pen's (first) side button is held.
    pub button: bool,
    /// How hard the pen presses, 0 to 1 (about 1 is a normal press).
    pub pressure: f32,
}

/// What a stylus or mouse event becomes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    Moved(PhysicalPosition<f64>),
    /// The left button, or the right one (`right`).
    Button { pressed: bool, right: bool },
    Left,
}

struct Shared {
    pen: StylusState,
    /// Until when fingers are still taken for the palm after the pen lifted.
    near_until: Option<Instant>,
    /// The press that is down went to the right button.
    right: bool,
    /// Fingers dropped as the palm, until they lift.
    rejected: Vec<i32>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    pen: StylusState { touching: false, hovering: false, eraser: false, button: false, pressure: 0.0 },
    near_until: None,
    right: false,
    rejected: Vec::new(),
});

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a stylus is doing now.
pub fn stylus() -> StylusState {
    shared().pen
}

/// A stylus, its eraser or a mouse: the tools that drive the cursor.
pub fn is_pointer_tool(tool: ToolType) -> bool {
    matches!(tool, ToolType::Stylus | ToolType::Eraser | ToolType::Mouse)
}

/// The cursor events for `motion` when it comes from a stylus or a mouse, else `None` (a touch).
pub fn pointer_events(motion: &MotionEvent<'_>) -> Option<Vec<PointerEvent>> {
    let action = motion.action();
    let pointer = match action {
        MotionAction::Down | MotionAction::Up | MotionAction::PointerDown | MotionAction::PointerUp => {
            let p = motion.pointer_at_index(motion.pointer_index());
            is_pointer_tool(p.tool_type()).then_some(p)?
        },
        _ => motion.pointers().find(|p| is_pointer_tool(p.tool_type()))?,
    };
    let tool = pointer.tool_type();
    let pen = tool != ToolType::Mouse;
    let pos = PhysicalPosition::new(pointer.x() as f64, pointer.y() as f64);
    let buttons = motion.button_state();
    let mut s = shared();
    if pen {
        s.pen.eraser = tool == ToolType::Eraser;
        s.pen.button = buttons.stylus_primary();
        s.pen.pressure = pointer.pressure();
    }
    let events = match action {
        MotionAction::HoverEnter | MotionAction::HoverMove => {
            if pen {
                s.pen.hovering = true;
                s.near_until = None;
            }
            vec![PointerEvent::Moved(pos)]
        },
        MotionAction::HoverExit => {
            if pen {
                s.pen.hovering = false;
                s.near_until = Some(Instant::now() + PALM_GRACE);
            }
            vec![PointerEvent::Moved(pos), PointerEvent::Left]
        },
        MotionAction::Down | MotionAction::PointerDown => {
            let right = if pen { buttons.stylus_primary() && !s.pen.eraser } else { buttons.secondary() };
            s.right = right;
            if pen {
                s.pen.touching = true;
                s.pen.hovering = false;
            }
            vec![PointerEvent::Moved(pos), PointerEvent::Button { pressed: true, right }]
        },
        MotionAction::Move => vec![PointerEvent::Moved(pos)],
        MotionAction::Up | MotionAction::PointerUp | MotionAction::Cancel => {
            if pen {
                s.pen.touching = false;
                s.pen.pressure = 0.0;
                s.near_until = Some(Instant::now() + PALM_GRACE);
            }
            vec![PointerEvent::Moved(pos), PointerEvent::Button { pressed: false, right: s.right }]
        },
        _ => Vec::new(),
    };
    Some(events)
}

/// Whether the finger `id` in this `phase` is the palm (to be dropped): it landed while the pen
/// was on or near the screen.
pub fn is_palm(id: i32, phase: TouchPhase) -> bool {
    let mut s = shared();
    let near = s.pen.touching || s.pen.hovering || s.near_until.is_some_and(|t| Instant::now() < t);
    match phase {
        TouchPhase::Started => {
            if near {
                s.rejected.push(id);
            }
            near
        },
        TouchPhase::Moved => s.rejected.contains(&id),
        TouchPhase::Ended | TouchPhase::Cancelled => match s.rejected.iter().position(|r| *r == id) {
            Some(i) => {
                s.rejected.remove(i);
                true
            },
            None => false,
        },
    }
}
