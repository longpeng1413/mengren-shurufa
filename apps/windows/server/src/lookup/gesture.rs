//! 认划词手势：左键按下到松开拖出一段距离，或在同一处快速连点（双击选词、三击选行）。
//! 只管鼠标轨迹；窗口有没有被拖着走由调用方比较前后位置后传进来。

/// 一次左键按下。
#[derive(Debug, Clone, Copy)]
struct Press {
    x: i32,

    y: i32,

    /// 系统时间（毫秒，会回绕）。
    at: u32,
}

#[derive(Debug)]
pub(super) struct Gesture {
    /// 拖出多远算拖选（物理像素）。
    drag: i32,

    /// 两次按下最多隔多久算连点（毫秒）。
    double_time: u32,

    /// 两次按下最多离多远算连点（物理像素）。
    double_slop: i32,

    /// 这次按下。
    down: Option<Press>,

    /// 上一次按下，认连点用。
    last: Option<Press>,

    /// 这次按下是不是连点里的第二下（或之后）。
    repeated: bool,
}

impl Gesture {
    pub(super) fn new(drag: i32, double_time: u32, double_slop: i32) -> Self {
        Self {
            drag,
            double_time,
            double_slop,
            down: None,
            last: None,
            repeated: false,
        }
    }

    pub(super) fn down(&mut self, x: i32, y: i32, at: u32) {
        let press = Press { x, y, at };
        self.repeated = self.last.is_some_and(|last| {
            at.wrapping_sub(last.at) <= self.double_time
                && (x - last.x).abs() <= self.double_slop
                && (y - last.y).abs() <= self.double_slop
        });
        self.last = Some(press);
        self.down = Some(press);
    }

    /// 松开左键：拖选或连点、且窗口没被拖着走，就该显示图标。
    pub(super) fn up(&mut self, x: i32, y: i32, window_moved: bool) -> bool {
        let Some(down) = self.down.take() else {
            return false;
        };
        let dragged = (x - down.x).abs() >= self.drag || (y - down.y).abs() >= self.drag;
        !window_moved && (dragged || self.repeated)
    }
}
