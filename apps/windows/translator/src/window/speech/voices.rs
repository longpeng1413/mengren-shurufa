//! 找本机某种语言的朗读声音：先看经典 SAPI 声音，再看系统新语音（OneCore）。
//!
//! 「设置 → 时间和语言 → 语音」里装的声音只登记在 `Speech_OneCore` 下，经典接口默认看不到；
//! 按这个类别枚举出来的声音令牌交给 `ISpVoice::SetVoice` 照样能读（2026-11-20 本机用康康实测）。
use windows::{
    Win32::{
        Media::Speech::{
            ISpObjectToken, ISpObjectTokenCategory, SPCAT_VOICES, SpObjectTokenCategory,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance},
    },
    core::{HSTRING, PCWSTR, Result, w},
};

/// 系统新语音的令牌类别。
const ONECORE_VOICES: PCWSTR =
    w!("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Speech_OneCore\\Voices");

/// 语言号（十六进制 LCID，如 `804`）对应的第一个声音；两类都没有回 `None`。
pub fn find(code: &str) -> Option<ISpObjectToken> {
    [SPCAT_VOICES, ONECORE_VOICES]
        .into_iter()
        .find_map(|category| first_token(category, code).ok().flatten())
}

fn first_token(category_id: PCWSTR, code: &str) -> Result<Option<ISpObjectToken>> {
    unsafe {
        let category: ISpObjectTokenCategory =
            CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        category.SetId(category_id, false)?;
        let tokens =
            category.EnumTokens(&HSTRING::from(format!("Language={code}")), PCWSTR::null())?;
        let mut count = 0;
        tokens.GetCount(&mut count)?;
        if count == 0 {
            return Ok(None);
        }
        Ok(Some(tokens.Item(0)?))
    }
}
