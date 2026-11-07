//! 自定义短语的同步收件箱。网络只写收件箱，编辑器只写本机库和隐藏标记，避免互相覆盖。

#[cfg(test)]
mod tests;

use crate::Phrase;
use qingjian_core::{CustomPhrase, storage::write_atomic_str};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PHRASES: usize = 10_000;

pub struct SyncedPhrases {
    directory: PathBuf,
}

impl SyncedPhrases {
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn id(phrase: &Phrase) -> String {
        let mut hash = Sha256::new();
        hash.update(phrase.code.as_bytes());
        hash.update([0]);
        hash.update(phrase.text.replace("\r\n", "\n").as_bytes());
        format!("{:x}", hash.finalize())
    }

    pub fn from_custom(&self, local: &[CustomPhrase]) -> std::io::Result<Vec<Phrase>> {
        let inbox: Vec<Phrase> = self.read("phrase-sync.json")?;
        let local: Vec<_> = local
            .iter()
            .map(|custom| {
                let mut phrase = Phrase::from_custom(custom);
                if let Some(old) = inbox.iter().find(|p| Self::id(p) == Self::id(&phrase)) {
                    phrase.group = old.group.clone();
                }
                phrase
            })
            .collect();
        self.merged(&local)
    }

    pub fn merged(&self, local: &[Phrase]) -> std::io::Result<Vec<Phrase>> {
        let hidden: BTreeSet<String> = self.read("phrase-sync-hidden.json")?;
        let inbox: Vec<Phrase> = self.read("phrase-sync.json")?;
        validate(&inbox)?;
        let mut ids = BTreeSet::new();
        Ok(local
            .iter()
            .chain(inbox.iter())
            .filter(|p| {
                let id = Self::id(p);
                !hidden.contains(&id) && ids.insert(id)
            })
            .take(MAX_PHRASES)
            .cloned()
            .collect())
    }

    /// 调用者用一把进程内互斥锁串行接收。所有条目先校验，失败不写任何文件。
    pub fn receive(&self, incoming: &[Phrase]) -> std::io::Result<usize> {
        validate(incoming)?;
        let mut inbox: Vec<Phrase> = self.read("phrase-sync.json")?;
        validate(&inbox)?;
        let mut ids: BTreeSet<_> = inbox.iter().map(Self::id).collect();
        let before = inbox.len();
        for phrase in incoming {
            if ids.insert(Self::id(phrase)) {
                inbox.push(phrase.clone());
            }
        }
        validate(&inbox)?;
        if inbox.len() != before {
            self.write("phrase-sync.json", &inbox)?;
        }
        Ok(inbox.len() - before)
    }

    /// 删除仅在本机隐藏，收到对方的旧快照不会把它恢复。
    pub fn hide(&self, removed: &[Phrase], present: &[Phrase]) -> std::io::Result<()> {
        let mut hidden: BTreeSet<String> = self.read("phrase-sync-hidden.json")?;
        hidden.extend(removed.iter().map(Self::id));
        for phrase in present {
            hidden.remove(&Self::id(phrase));
        }
        if hidden.len() > 100_000 {
            return Err(invalid());
        }
        self.write("phrase-sync-hidden.json", &hidden)
    }

    fn read<T: serde::de::DeserializeOwned + Default>(&self, name: &str) -> std::io::Result<T> {
        let path = self.directory.join(name);
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
            Err(e) => return Err(e),
        };
        if file.metadata()?.len() > MAX_BYTES {
            return Err(invalid());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        serde_json::from_slice(&bytes).map_err(|_| invalid())
    }

    fn write(&self, name: &str, value: &impl serde::Serialize) -> std::io::Result<()> {
        let text = serde_json::to_string(value).map_err(|_| invalid())?;
        if text.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        std::fs::create_dir_all(&self.directory)?;
        write_atomic_str(&self.directory.join(name), &text)
    }
}

pub fn merged_custom_phrases(directory: &Path, local: &[CustomPhrase]) -> Vec<CustomPhrase> {
    match SyncedPhrases::new(directory).from_custom(local) {
        Ok(phrases) => phrases
            .iter()
            .filter(|p| qingjian_core::custom_phrase::valid_code(&p.code))
            .map(|p| CustomPhrase {
                code: p.code.clone(),
                text: p.text.clone(),
                weight: p.weight,
                enabled: p.enabled,
            })
            .collect(),
        Err(_) => local.to_vec(),
    }
}

pub fn validate(phrases: &[Phrase]) -> std::io::Result<()> {
    if phrases.len() > MAX_PHRASES {
        return Err(invalid());
    }
    for p in phrases {
        if p.text.trim().is_empty()
            || p.text.len() > 64 * 1024
            || p.group.len() > 256
            || !(1..=999).contains(&p.weight)
            || (!p.code.is_empty() && !qingjian_core::custom_phrase::valid_code(&p.code))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn invalid() -> std::io::Error {
    std::io::Error::other("Invalid or oversized custom phrase data")
}
