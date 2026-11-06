//! Fit the main window to the screen it opens on.
//!
//! The window asks for 1440 x 900 (tauri.conf.json). On a smaller screen, or one with display
//! scaling (a 1920 x 1080 laptop at 150 % has about 1280 x 680 above the taskbar), that window ran
//! past the bottom of the screen, behind the taskbar, and hid Settings at the foot of the sidebar.
//! At start-up the window is shrunk to the screen's usable area and centred in it. A window that
//! already fits is left alone.

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// The outer frame the window should have to fit inside `work` (the screen without the taskbar),
/// or `None` when it already fits. Never smaller than `min` (the window's minimum outer size); if
/// even that is too big, it starts at the work area's top-left so the title bar stays reachable.
pub fn fit(window: Rect, work: Rect, min: (u32, u32)) -> Option<Rect> {
    let right = |r: Rect| r.x as i64 + r.w as i64;
    let bottom = |r: Rect| r.y as i64 + r.h as i64;
    if window.x >= work.x && window.y >= work.y && right(window) <= right(work) && bottom(window) <= bottom(work) {
        return None;
    }
    let w = window.w.min(work.w).max(min.0);
    let h = window.h.min(work.h).max(min.1);
    Some(Rect {
        x: work.x + (work.w.saturating_sub(w) / 2) as i32,
        y: work.y + (work.h.saturating_sub(h) / 2) as i32,
        w,
        h,
    })
}

/// Apply `fit` to the main window. Best effort: the caller ignores a failure, and the window then
/// keeps the size it was created with.
#[cfg(desktop)]
pub fn fit_main_window(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else { return Ok(()) };
    let Some(monitor) = win.current_monitor()? else { return Ok(()) };
    let area = monitor.work_area();
    let (pos, outer, inner, scale) = (win.outer_position()?, win.outer_size()?, win.inner_size()?, win.scale_factor()?);
    // The title bar and borders: what the outer frame adds to the page area.
    let frame = (outer.width.saturating_sub(inner.width), outer.height.saturating_sub(inner.height));
    let config = app.config().app.windows.iter().find(|w| w.label == "main");
    let min_inner = |v: Option<f64>| (v.unwrap_or(0.0) * scale).round() as u32;
    let min = (
        min_inner(config.and_then(|c| c.min_width)) + frame.0,
        min_inner(config.and_then(|c| c.min_height)) + frame.1,
    );
    let window = Rect { x: pos.x, y: pos.y, w: outer.width, h: outer.height };
    let work = Rect { x: area.position.x, y: area.position.y, w: area.size.width, h: area.size.height };
    if let Some(r) = fit(window, work, min) {
        win.set_size(tauri::PhysicalSize::new(r.w - frame.0, r.h - frame.1))?;
        win.set_position(tauri::PhysicalPosition::new(r.x, r.y))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{fit, Rect};

    const MIN: (u32, u32) = (1040, 690);

    fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_window_that_fits_is_left_alone() {
        let work = r(0, 0, 2560, 1400);
        assert_eq!(fit(r(560, 250, 1440, 900), work, MIN), None);
        assert_eq!(fit(r(0, 0, 2560, 1400), work, MIN), None, "exactly the work area");
    }

    #[test]
    fn a_window_taller_than_the_screen_is_shrunk_and_centred() {
        // The test VM: about 2000 x 1140 above the taskbar, a 2160 x 1350 window at 150 %.
        let work = r(0, 0, 2000, 1140);
        assert_eq!(fit(r(0, 0, 2160, 1350), work, MIN), Some(r(0, 0, 2000, 1140)));
        // Only too tall: the width is kept and the window centred across.
        assert_eq!(fit(r(0, 0, 1500, 1350), work, MIN), Some(r(250, 0, 1500, 1140)));
        // Only too wide.
        assert_eq!(fit(r(0, 0, 2400, 900), work, MIN), Some(r(0, 120, 2000, 900)));
    }

    #[test]
    fn a_window_that_hangs_off_the_screen_is_moved_back_at_its_size() {
        let work = r(0, 0, 2560, 1400);
        assert_eq!(fit(r(1800, 900, 1440, 900), work, MIN), Some(r(560, 250, 1440, 900)));
        assert_eq!(fit(r(-50, 10, 1440, 900), work, MIN), Some(r(560, 250, 1440, 900)));
    }

    #[test]
    fn the_work_area_need_not_start_at_zero() {
        // A taskbar on the left, one at the top, and a second screen to the left of the main one.
        assert_eq!(fit(r(0, 0, 2160, 1350), r(96, 0, 1824, 1140), MIN), Some(r(96, 0, 1824, 1140)));
        assert_eq!(fit(r(0, 0, 1440, 900), r(0, 72, 2000, 1068), MIN), Some(r(280, 156, 1440, 900)));
        assert_eq!(fit(r(-1920, -40, 1440, 900), r(-1920, 0, 1920, 1040), MIN), Some(r(-1680, 70, 1440, 900)));
    }

    #[test]
    fn it_never_goes_below_the_minimum_size_and_keeps_the_title_bar_on_screen() {
        // 1366 x 768 at 150 %: smaller than the minimum window.
        let work = r(0, 0, 1366, 720);
        assert_eq!(fit(r(0, 0, 2160, 1350), work, (1560, 1035)), Some(r(0, 0, 1560, 1035)));
    }
}

#[cfg(test)]
mod wiring_tests {
    /// The window step needs a running app, so the host checks it is called at start-up, on
    /// desktop only, and that a failure there cannot stop Nakos from opening.
    #[test]
    fn start_up_fits_the_main_window_on_desktop() {
        let lib = include_str!("lib.rs");
        let setup = &lib[lib.find(".setup(|app| {").expect("a setup hook")..];
        let setup = &setup[..setup.find("})").unwrap()];
        let init = setup.find("state::init(app)?;").expect("state is set up first");
        let call = setup.find("#[cfg(desktop)]\n            let _ = window_fit::fit_main_window(app);").expect("the window is fitted, best effort, on desktop");
        assert!(init < call, "{setup}");
        let module = include_str!("window_fit.rs");
        let apply = &module[module.find("pub fn fit_main_window(").unwrap()..];
        let apply = &apply[..apply.find("\n}\n").unwrap()];
        assert!(apply.contains("if let Some(r) = fit(window, work, min) {"), "{apply}");
        assert!(apply.contains("app.config().app.windows"), "the minimum comes from the config: {apply}");
    }
}
