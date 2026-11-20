//! 联网朗读：百度翻译网页的发音（`fanyi.baidu.com/gettts`，不要密钥），本机没有这门语言的声音时用。
//!
//! 这不是百度公开的接口，随时可能失效；实测只给中、英、日、韩出声，法德西俄越回 403（2026-11-20）。
//! 文字按句切成小段，后台线程逐段下载，下好一段就投递给窗口；播放走 MCI（`mpegvideo`），
//! 可直接设倍速，与系统朗读共用一个档位。音频只落一个临时文件，播完、停止、退出与下次启动时都删掉。
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Multimedia::mciSendStringW;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::HSTRING;

/// 后台线程下好一段（或失败、结束）时投递给窗口的消息，见窗口的 `VOICE_IN`。
pub const VOICE_IN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;

/// MCI 播完一段后发给窗口的通知。
pub const MM_MCINOTIFY: u32 = 0x3B9;

/// `MM_MCINOTIFY` 的 wParam：正常播完。
const MCI_NOTIFY_SUCCESSFUL: usize = 1;

/// MCI 设备别名。
const ALIAS: &str = "mengren_translator_voice";

/// 每段最多多少字（按句切，太长的句子硬切）。
const CHUNK_CHARS: usize = 150;

/// 每段下载失败时的重试次数。
const RETRIES: usize = 2;

/// 后台线程交回的一段结果。
enum Piece {
    Audio(Vec<u8>),
    Failed,
    End,
}

/// 后台线程与窗口之间的信箱：代次 + 结果。
type Inbox = Arc<Mutex<VecDeque<(u64, Piece)>>>;

/// 联网朗读的结果，窗口据此改状态行。
pub enum Outcome {
    /// 一段都没取到。
    Failed,
}

pub struct Online {
    /// 窗口句柄（数值），后台线程用来投递消息。
    hwnd: isize,

    /// 当前这次朗读的代次；停止或重读时加一，旧线程交回的结果作废。
    generation: Arc<AtomicU64>,

    inbox: Inbox,

    /// 已下好、等着播的段。
    queue: VecDeque<Vec<u8>>,

    /// 正在播一段。
    playing: bool,

    /// 后台线程已下完（或失败）。
    fetched: bool,

    /// 这次朗读是否播出过声音。
    played_any: bool,

    /// 这次朗读还没结束。
    active: bool,

    /// MCI 倍速，1000 为原速。
    speed: u32,
}

/// 百度能读的语言及其语言参数。
pub fn code(lang: &str) -> Option<&'static str> {
    match lang {
        "zh" => Some("zh"),
        "en" => Some("en"),
        "ja" => Some("jp"),
        "ko" => Some("kor"),
        _ => None,
    }
}

impl Online {
    pub fn new(hwnd: HWND) -> Self {
        remove_file();
        Self {
            hwnd: hwnd.0 as isize,
            generation: Arc::new(AtomicU64::new(0)),
            inbox: Arc::default(),
            queue: VecDeque::new(),
            playing: false,
            fetched: false,
            played_any: false,
            active: false,
            speed: 1000,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// 换倍速；正在播的这一段也跟着变。
    pub fn set_speed(&mut self, speed: u32) {
        self.speed = speed;
        if self.playing {
            mci(&format!("set {ALIAS} speed {speed}"));
        }
    }

    /// 开始联网朗读 `text`（`lang` 须是 [`code`] 认得的语言）。
    pub fn start(&mut self, text: &str, lang: &str) {
        self.stop();
        let Some(lan) = code(lang) else {
            return;
        };
        let chunks = chunks(text);
        if chunks.is_empty() {
            return;
        }
        self.active = true;
        let generation = self.generation.load(Ordering::SeqCst);
        let current = self.generation.clone();
        let inbox = self.inbox.clone();
        let hwnd = self.hwnd;
        let spawned = std::thread::Builder::new()
            .name("translator-online-voice".to_owned())
            .spawn(move || fetch(&chunks, lan, generation, &current, &inbox, hwnd));
        if let Err(error) = spawned {
            tracing::warn!(%error, "online voice thread could not start");
            self.active = false;
        }
    }

    /// 窗口收到 `VOICE_IN`：取走本次的结果，没在播就开播。
    pub fn ready(&mut self) -> Option<Outcome> {
        let generation = self.generation.load(Ordering::SeqCst);
        let pieces: Vec<Piece> = {
            let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox
                .drain(..)
                .filter(|(g, _)| *g == generation)
                .map(|(_, piece)| piece)
                .collect()
        };
        let mut outcome = None;
        for piece in pieces {
            match piece {
                Piece::Audio(bytes) => self.queue.push_back(bytes),
                Piece::Failed => {
                    self.fetched = true;
                    if !self.played_any && self.queue.is_empty() {
                        outcome = Some(Outcome::Failed);
                    }
                }
                Piece::End => self.fetched = true,
            }
        }
        if self.active && !self.playing {
            self.play_next();
        }
        outcome
    }

    /// 窗口收到 `MM_MCINOTIFY`：正常播完一段就接着播下一段。
    pub fn notify(&mut self, flag: usize) {
        if !self.active || flag != MCI_NOTIFY_SUCCESSFUL {
            return;
        }
        self.playing = false;
        self.play_next();
    }

    pub fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.playing || self.active {
            mci(&format!("close {ALIAS}"));
        }
        self.queue.clear();
        self.playing = false;
        self.fetched = false;
        self.played_any = false;
        self.active = false;
        remove_file();
    }

    fn play_next(&mut self) {
        let Some(bytes) = self.queue.pop_front() else {
            if self.fetched {
                // 都播完了
                self.stop();
            }
            return;
        };
        let Some(path) = file_path() else {
            self.stop();
            return;
        };
        mci(&format!("close {ALIAS}"));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(error) = std::fs::write(&path, bytes) {
            tracing::warn!(%error, "online voice audio could not be written");
            self.stop();
            return;
        }
        let opened = mci(&format!(
            "open \"{}\" type mpegvideo alias {ALIAS}",
            path.display()
        ));
        if !opened {
            self.stop();
            return;
        }
        mci(&format!("set {ALIAS} speed {}", self.speed));
        let command = HSTRING::from(format!("play {ALIAS} notify"));
        let error = unsafe { mciSendStringW(&command, None, Some(HWND(self.hwnd as _))) };
        if error != 0 {
            tracing::warn!(error, "online voice playback failed");
            self.stop();
            return;
        }
        self.playing = true;
        self.played_any = true;
    }
}

impl Drop for Online {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 发一条 MCI 命令，成功回 `true`。
fn mci(command: &str) -> bool {
    unsafe { mciSendStringW(&HSTRING::from(command), None, None) == 0 }
}

/// 临时音频 `%LOCALAPPDATA%\Qingjian\cache\translator-voice.mp3`。
fn file_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| {
        PathBuf::from(base)
            .join("Qingjian")
            .join("cache")
            .join("translator-voice.mp3")
    })
}

fn remove_file() {
    if let Some(path) = file_path()
        && path.exists()
    {
        let _ = std::fs::remove_file(path);
    }
}

/// 后台线程：逐段下载，下好一段投递一次；停止或重读后（代次变了）不再继续。
fn fetch(
    chunks: &[String],
    lan: &str,
    generation: u64,
    current: &AtomicU64,
    inbox: &Inbox,
    hwnd: isize,
) {
    let deliver = |piece: Piece| {
        inbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back((generation, piece));
        let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), VOICE_IN, WPARAM(0), LPARAM(0)) };
    };
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Mozilla/5.0")
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "online voice client could not be built");
            deliver(Piece::Failed);
            return;
        }
    };
    for chunk in chunks {
        if current.load(Ordering::SeqCst) != generation {
            return;
        }
        let url = format!(
            "https://fanyi.baidu.com/gettts?lan={lan}&text={}&spd=5&source=web",
            encode(chunk)
        );
        let audio = (0..=RETRIES).find_map(|_| download(&client, &url));
        match audio {
            Some(bytes) => deliver(Piece::Audio(bytes)),
            None => {
                tracing::warn!(lan, "online voice returned no audio");
                deliver(Piece::Failed);
                return;
            }
        }
    }
    deliver(Piece::End);
}

/// 下一段音频；不是音频（403 网页等）回 `None`。
fn download(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    let response = client.get(url).send().ok()?;
    let audio = response.status().is_success()
        && response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("audio/"));
    if !audio {
        return None;
    }
    let bytes = response.bytes().ok()?;
    (!bytes.is_empty()).then(|| bytes.to_vec())
}

/// 网址参数编码（UTF-8 字节，保留字母数字与 `-_.~`）。
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 按句切段：遇到句末标点或换行断开，单句超过 [`CHUNK_CHARS`] 再硬切；空白段丢掉。
fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        let end = matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n');
        if end || current.chars().count() >= CHUNK_CHARS {
            out.push(std::mem::take(&mut current));
        }
    }
    out.push(current);
    out.into_iter()
        .map(|chunk| chunk.trim().to_owned())
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{CHUNK_CHARS, chunks, code, encode};

    #[test]
    fn splits_sentences_and_long_runs() {
        assert_eq!(chunks("你好。今天好！\n  "), ["你好。", "今天好！"]);
        let long = "字".repeat(CHUNK_CHARS + 10);
        let parts = chunks(&long);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].chars().count(), CHUNK_CHARS);
    }

    #[test]
    fn encodes_utf8_and_reserved_characters() {
        assert_eq!(encode("a b&你"), "a%20b%26%E4%BD%A0");
    }

    #[test]
    fn only_languages_baidu_reads() {
        assert_eq!(code("ja"), Some("jp"));
        assert_eq!(code("ko"), Some("kor"));
        assert_eq!(code("fr"), None);
        assert_eq!(code("vi"), None);
    }
}
