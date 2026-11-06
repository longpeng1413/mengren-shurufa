//! 录音文件识别 2.0：录音留在内存，说完后上传，按任务编号查询结果。

use super::{VolcengineConfig, command::Command};
use crate::VoiceError;
use base64::{Engine, engine::general_purpose::STANDARD};
use qingjian_core::TranscriptEvent;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError, Sender},
};
use std::time::{Duration, Instant};

pub fn run(
    config: VolcengineConfig,
    rate: u32,
    commands: Receiver<Command>,
    events: Sender<TranscriptEvent>,
    cancelled: Arc<AtomicBool>,
) {
    let result = recognize(config, rate, commands, &cancelled);
    if cancelled.load(Ordering::Relaxed) {
        return;
    }
    let event = match result {
        Ok(text) => TranscriptEvent::Final(text),
        Err(error) => TranscriptEvent::Error(error.describe()),
    };
    let _ = events.send(event);
}

fn recognize(
    config: VolcengineConfig,
    rate: u32,
    commands: Receiver<Command>,
    cancelled: &AtomicBool,
) -> Result<String, VoiceError> {
    let mut samples = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(String::new());
        }
        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Audio(audio)) => {
                if samples.len().saturating_add(audio.len()) > rate as usize * 300 {
                    return Err(VoiceError::Protocol("recording exceeds five minutes"));
                }
                samples.extend(audio);
            }
            Ok(Command::Finish) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(String::new()),
        }
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| VoiceError::Connect("cannot start request runtime".to_owned()))?;
    runtime.block_on(async {
        let client = crate::tls::http_client()?;
        let id = uuid::Uuid::new_v4().to_string();
        let body = json!({"user":{"uid":"mengren"},"audio":{"data": STANDARD.encode(wav(&samples, rate)), "format":"wav", "rate":rate, "bits":16, "channel":1},"request":{"model_name":"bigmodel", "enable_itn":true,"enable_punc":config.punctuation}});
        let submit = request(&client, "submit", &config, &id).header("X-Api-Sequence", "-1").json(&body).send().await.map_err(|_| VoiceError::Connect("submission failed".to_owned()))?;
        check(&submit)?;
        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Relaxed) { return Ok(String::new()); }
            if started.elapsed() > Duration::from_secs(180) { return Err(VoiceError::Timeout(180_000)); }
            tokio::time::sleep(Duration::from_secs(2)).await;
            if cancelled.load(Ordering::Relaxed) { return Ok(String::new()); }
            let reply = request(&client, "query", &config, &id).json(&json!({})).send().await.map_err(|_| VoiceError::Connect("query failed".to_owned()))?;
            let code = check(&reply)?;
            match code.as_str() {
                "20000000" => {
                    let body: Value = reply.json().await.map_err(|_| VoiceError::Protocol("invalid recognition response"))?;
                    return body.pointer("/result/text").and_then(Value::as_str).map(str::to_owned).ok_or(VoiceError::Protocol("missing recognized text"));
                },
                "20000003" => return Ok(String::new()),
                _ => {},
            }
        }
    })
}

fn request(
    client: &reqwest::Client,
    action: &str,
    config: &VolcengineConfig,
    id: &str,
) -> reqwest::RequestBuilder {
    client
        .post(format!(
            "https://openspeech.bytedance.com/api/v3/auc/bigmodel/{action}"
        ))
        .header("X-Api-Key", &config.api_key)
        .header("X-Api-Resource-Id", VolcengineConfig::FILE_2)
        .header("X-Api-Request-Id", id)
}

fn check(reply: &reqwest::Response) -> Result<String, VoiceError> {
    if !reply.status().is_success() {
        return Err(VoiceError::Rejected(reply.status().as_u16()));
    }
    let code = reply
        .headers()
        .get("X-Api-Status-Code")
        .and_then(|v| v.to_str().ok())
        .ok_or(VoiceError::Protocol("missing recognition status"))?;
    if !matches!(code, "20000000" | "20000001" | "20000002" | "20000003") {
        return Err(VoiceError::Server {
            code: code.parse().unwrap_or(0),
            message: "recognition request rejected".to_owned(),
        });
    }
    Ok(code.to_owned())
}

fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let length = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(length as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_distinguishes_pending_auth_failure_and_completion() {
        let response = |status, code| {
            reqwest::Response::from(
                tungstenite::http::Response::builder()
                    .status(status)
                    .header("X-Api-Status-Code", code)
                    .body(Vec::<u8>::new())
                    .unwrap(),
            )
        };
        assert_eq!(
            super::check(&response(200, "20000001")).unwrap(),
            "20000001"
        );
        assert_eq!(
            super::check(&response(200, "20000000")).unwrap(),
            "20000000"
        );
        assert!(super::check(&response(401, "45000001")).is_err());
        assert!(super::check(&response(200, "45000001")).is_err());
    }
    #[test]
    fn wav_contains_rate_length_and_pcm() {
        let bytes = super::wav(&[1, -2], 16000);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[24..28], &16000u32.to_le_bytes());
        assert_eq!(&bytes[40..44], &4u32.to_le_bytes());
        assert_eq!(&bytes[44..], &[1, 0, 254, 255]);
    }
}
