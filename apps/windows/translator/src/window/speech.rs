//! SAPI 异步朗读及语言声音选择。
use super::translation::LANGS;
use windows::{
    Win32::{
        Media::Speech::{
            ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPRS_IS_SPEAKING, SPVOICESTATUS,
            SpObjectTokenCategory, SpVoice,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance},
    },
    core::{HSTRING, PCWSTR, Result},
};
#[derive(Default)]
pub struct Speech {
    voice: Option<ISpVoice>,
    last: Option<usize>,
}
impl Speech {
    pub fn stop(&mut self) {
        if let Some(v) = &self.voice {
            let _ = unsafe { v.Speak(PCWSTR::null(), 3, None) };
        }
        self.last = None;
    }
    pub fn read(&mut self, id: usize, text: &str, lang: &str) -> Result<()> {
        if text.is_empty() {
            self.stop();
            return Ok(());
        }
        if self.voice.is_none() {
            self.voice = Some(unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }?);
        }
        let voice = self.voice.as_ref().unwrap();
        let mut state = SPVOICESTATUS::default();
        if self.last == Some(id)
            && unsafe { voice.GetStatus(&mut state, std::ptr::null_mut()) }.is_ok()
            && state.dwRunningState == SPRS_IS_SPEAKING.0 as u32
        {
            self.stop();
            return Ok(());
        }
        let code = LANGS.iter().find(|l| l.0 == lang).map_or("804", |l| l.2);
        let has_voice = (|| -> Result<bool> {
            unsafe {
                let category: ISpObjectTokenCategory =
                    CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
                category.SetId(SPCAT_VOICES, false)?;
                let tokens = category
                    .EnumTokens(&HSTRING::from(format!("Language={code}")), PCWSTR::null())?;
                let mut n = 0;
                tokens.GetCount(&mut n)?;
                Ok(n > 0)
            }
        })()
        .unwrap_or(false);
        let safe = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let xml = format!("<voice required=\u{22}Language={code}\u{22}>{safe}</voice>");
        if !has_voice || unsafe { voice.Speak(&HSTRING::from(xml), 11, None) }.is_err() {
            unsafe { voice.Speak(&HSTRING::from(text), 3 | 16, None) }?;
        }
        self.last = Some(id);
        Ok(())
    }
}
