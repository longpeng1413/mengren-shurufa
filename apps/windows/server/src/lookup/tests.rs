use super::gesture::Gesture;

fn gesture() -> Gesture {
    Gesture::new(8, 500, 4)
}

#[test]
fn a_drag_shows_the_icon_but_a_plain_click_does_not() {
    let mut g = gesture();
    g.down(100, 100, 1_000);
    assert!(g.up(160, 104, false));
    g.down(300, 300, 5_000);
    assert!(!g.up(302, 301, false));
}

#[test]
fn dragging_a_window_does_not_count() {
    let mut g = gesture();
    g.down(100, 100, 1_000);
    assert!(!g.up(400, 300, true));
}

#[test]
fn a_double_click_counts_only_when_close_in_time_and_place() {
    let mut g = gesture();
    g.down(50, 50, 1_000);
    assert!(!g.up(50, 50, false));
    g.down(51, 50, 1_200);
    assert!(g.up(51, 50, false), "double click selects a word");
    g.down(51, 50, 1_400);
    assert!(g.up(51, 50, false), "triple click selects a line");
    g.down(51, 50, 3_000);
    assert!(!g.up(51, 50, false), "too late");
    g.down(90, 50, 3_100);
    assert!(!g.up(90, 50, false), "too far");
}

#[test]
fn a_release_without_a_press_is_ignored() {
    let mut g = gesture();
    assert!(!g.up(10, 10, false));
}

#[test]
fn the_tick_count_may_wrap() {
    let mut g = gesture();
    g.down(5, 5, u32::MAX - 100);
    assert!(!g.up(5, 5, false));
    g.down(5, 5, 100);
    assert!(g.up(5, 5, false));
}

#[test]
fn screen_points_survive_packing_including_negative_monitors() {
    for (x, y) in [(0, 0), (1920, 1080), (-1280, 300), (3839, -200)] {
        assert_eq!(super::unpack(super::pack(x, y)), (x, y));
    }
}

mod hover {
    use std::time::{Duration, Instant};

    use qingjian_platform::HoverMode;

    use super::super::hover::{Dwell, Sample};

    fn at(x: i32, y: i32, ctrl: bool) -> Sample {
        Sample {
            x,
            y,
            ctrl,
            button: false,
        }
    }

    fn ms(start: Instant, value: u64) -> Instant {
        start + Duration::from_millis(value)
    }

    #[test]
    fn ctrl_mode_fires_once_after_a_short_rest_with_ctrl_held() {
        let mut dwell = Dwell::default();
        dwell.set_mode(HoverMode::Ctrl);
        let t = Instant::now();
        assert_eq!(dwell.tick(at(100, 100, false), ms(t, 0)), None);
        assert_eq!(dwell.tick(at(100, 100, false), ms(t, 900)), None, "no ctrl");
        assert_eq!(dwell.tick(at(100, 100, true), ms(t, 1000)), None);
        assert_eq!(dwell.tick(at(101, 102, true), ms(t, 1100)), None);
        assert_eq!(
            dwell.tick(at(101, 101, true), ms(t, 1300)),
            None,
            "Ctrl+C 常在按下 Ctrl 后 0.3 秒内按 C，这时还不弹"
        );
        assert_eq!(
            dwell.tick(at(101, 101, true), ms(t, 1500)),
            Some((100, 100))
        );
        assert_eq!(
            dwell.tick(at(101, 101, true), ms(t, 1800)),
            None,
            "once per rest"
        );
        assert_eq!(
            dwell.tick(at(160, 101, true), ms(t, 1900)),
            None,
            "moved away"
        );
        assert_eq!(
            dwell.tick(at(160, 101, true), ms(t, 2400)),
            Some((160, 101))
        );
    }

    #[test]
    fn always_mode_waits_longer_and_buttons_cancel() {
        let mut dwell = Dwell::default();
        dwell.set_mode(HoverMode::Always);
        let t = Instant::now();
        assert_eq!(dwell.tick(at(5, 5, false), ms(t, 0)), None);
        assert_eq!(dwell.tick(at(5, 5, false), ms(t, 300)), None);
        let mut pressed = at(5, 5, false);
        pressed.button = true;
        assert_eq!(dwell.tick(pressed, ms(t, 600)), None, "dragging");
        assert_eq!(dwell.tick(at(5, 5, false), ms(t, 700)), None);
        assert_eq!(dwell.tick(at(5, 5, false), ms(t, 1250)), Some((5, 5)));
    }

    #[test]
    fn off_never_fires() {
        let mut dwell = Dwell::default();
        let t = Instant::now();
        for step in 0..20 {
            assert_eq!(dwell.tick(at(1, 1, true), ms(t, step * 100)), None);
        }
    }
}

/// 手动跑：`MENGREN_PROBE=x,y cargo test -p qingjian-windows-server --lib probe_word_at -- --ignored --nocapture`，
/// 看屏幕上这一点取到的词（辅助接口优先、只用截图识字两种）。
#[test]
#[ignore = "reads the live screen at MENGREN_PROBE=x,y"]
fn probe_word_at() {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    let point = std::env::var("MENGREN_PROBE").expect("MENGREN_PROBE=x,y");
    let (x, y) = point.split_once(',').expect("x,y");
    let (x, y): (i32, i32) = (x.trim().parse().unwrap(), y.trim().parse().unwrap());
    println!("automation or OCR: {:?}", super::point::word_at(x, y));
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    println!("OCR only: {:?}", super::ocr::word_at(x, y));
}
