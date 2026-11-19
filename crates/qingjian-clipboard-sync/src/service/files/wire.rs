//! 带界限的数据帧，结束帧后用总长度和 SHA-256 校验，包括零字节文件。
use super::FILE_LIMIT;
use crate::SyncError;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

pub(super) const READY: &[u8] = b"lan-file-receiver-v1";
const CHUNK: usize = 256 * 1024;

fn invalid() -> SyncError {
    SyncError::Protocol("invalid file length or digest".into())
}

fn trailer(length: u64, digest: Sha256) -> Vec<u8> {
    let mut result = length.to_be_bytes().to_vec();
    result.extend(digest.finalize());
    result
}

pub(super) fn send(
    mut input: impl Read,
    length: Option<u64>,
    mut emit: impl FnMut(&[u8]) -> Result<(), SyncError>,
) -> Result<(), SyncError> {
    if length.is_some_and(|n| n >= FILE_LIMIT) {
        return Err(invalid());
    }
    let mut buffer = vec![0; CHUNK];
    let mut total = 0;
    let mut hash = Sha256::new();
    loop {
        let max = (FILE_LIMIT - total).min(CHUNK as u64) as usize;
        let n = input.read(&mut buffer[..max])?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total >= FILE_LIMIT || length.is_some_and(|len| total > len) {
            return Err(invalid());
        }
        hash.update(&buffer[..n]);
        emit(&buffer[..n])?;
    }
    if length.is_some_and(|len| len != total) {
        return Err(invalid());
    }
    emit(&[])?;
    emit(&trailer(total, hash))
}

pub(super) fn receive(
    mut output: impl Write,
    length: Option<u64>,
    mut next: impl FnMut(usize) -> Result<Vec<u8>, SyncError>,
) -> Result<(), SyncError> {
    let mut total = 0;
    let mut hash = Sha256::new();
    loop {
        let data = next(CHUNK)?;
        if data.is_empty() {
            break;
        }
        total += data.len() as u64;
        if data.len() > CHUNK || total >= FILE_LIMIT || length.is_some_and(|len| total > len) {
            return Err(invalid());
        }
        output.write_all(&data)?;
        hash.update(&data);
    }
    if length.is_some_and(|len| len != total) || next(40)? != trailer(total, hash) {
        return Err(invalid());
    }
    output.flush()?;
    Ok(())
}
