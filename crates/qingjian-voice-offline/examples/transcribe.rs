//! 在电脑上验证离线识别：`cargo run -p qingjian-voice-offline --example transcribe -- <模型根目录> <引擎库目录> <16 位单声道 WAV>`。
//! 先整段识别一遍，再按 0.1 秒一截模拟边说边喂，打印中间结果与最终结果。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use qingjian_core::{Transcriber, TranscriptEvent};
use qingjian_voice_offline::{OfflineConfig, OfflineTranscriber, SpeechModel};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, library, wav] = args.as_slice() else {
        eprintln!("usage: transcribe <model-root> <library-dir> <wav>");
        std::process::exit(2);
    };
    let config = OfflineConfig::new(SpeechModel::new(root), Some(PathBuf::from(library)));
    println!(
        "library version: {:?}",
        qingjian_voice_offline::library_version(&config)
    );
    let (rate, samples) = read_wav(wav);
    println!(
        "audio: {rate} Hz, {:.1} s",
        samples.len() as f32 / rate as f32
    );

    let started = Instant::now();
    let text = qingjian_voice_offline::transcribe(&config, rate, &samples);
    println!("one-shot ({} ms): {text:?}", started.elapsed().as_millis());

    let mut transcriber = OfflineTranscriber::new(config);
    println!("available: {}", transcriber.available());
    transcriber.start(rate);
    let chunk = rate as usize / 10;
    let started = Instant::now();
    for piece in samples.chunks(chunk) {
        transcriber.feed(piece);
        std::thread::sleep(Duration::from_millis(100));
        while let Some(event) = transcriber.poll() {
            println!("{:>6} ms {event:?}", started.elapsed().as_millis());
            if !matches!(event, TranscriptEvent::Partial(_)) {
                return;
            }
        }
    }
    transcriber.finish();
    let finished = Instant::now();
    while finished.elapsed() < Duration::from_secs(60) {
        match transcriber.poll() {
            Some(TranscriptEvent::Partial(text)) => println!("partial {text:?}"),
            Some(event) => {
                println!(
                    "{event:?} ({} ms after finish)",
                    finished.elapsed().as_millis()
                );
                break;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// 只认 PCM 16 位：找 `fmt ` 与 `data` 块，多声道取第一声道。
fn read_wav(path: &str) -> (u32, Vec<i16>) {
    let bytes = std::fs::read(path).expect("read wav");
    let mut at = 12;
    let (mut rate, mut channels) = (16_000u32, 1usize);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        if id == b"fmt " {
            channels = usize::from(u16::from_le_bytes([body[2], body[3]]));
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
        } else if id == b"data" {
            let samples = body
                .chunks_exact(2 * channels)
                .map(|frame| i16::from_le_bytes([frame[0], frame[1]]))
                .collect();
            return (rate, samples);
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk");
}
