---
title: The settings window
order: 4
description: "How to open Settings, search and the ⓘ notes, what each of the 12 pages controls, plus the macOS input menu and the legacy settings."
---

The settings window is currently in Chinese only. On this site, its labels are written in Chinese with an English translation in parentheses, so you can match them to what you see.

## Opening Settings

- **macOS**: 设置… (Settings…) in the input method menu (the 中 / 英 item in the menu bar, or Qingjian's entries in the system input source menu).
- **Windows**: 青简设置 (Qingjian Settings) in the Start menu, or the ⚙ on the floating status bar.
- **Linux**: 青简设置 (Qingjian Settings) in the application menu.

Changes take effect immediately: there's no Save button and no need to switch input methods again.

The search box at the top left finds settings by name, option text or words in their descriptions; for example, searching 英语 (English) finds 译文 (Gloss). Clicking a result (or pressing `Enter` for the first one) jumps to its page and marks the setting; `Esc` clears the search.
The ⓘ next to a heading shows a short note when you hover over it and opens the matching section of this documentation when you click it.

Settings that exist only on some platforms are hidden on the others; settings that depend on another setting (for example, fonts only apply to Qingjian's own renderer) are greyed out.

## What each page controls

| Page | Contents | Details |
|---|---|---|
| 输入 (Input) | Pinyin scheme (full pinyin, double pinyin, Zhuyin), shape-based input (Wubi), picking in steps, key hints, fuzzy pinyin, auxiliary code | [Pinyin schemes and fuzzy pinyin](../input/fuzzy-and-shuangpin.md), [Wubi and mixed input](../input/wubi.md), [Auxiliary code](../input/aux-code.md) |
| 候选 (Candidates) | Candidates per page, numpad digits (Windows), learning language, part of speech, Traditional Chinese, pinyin with tones, emoji and symbols, learning your habits, local sentence model, English words | [Glosses beside candidates](../learning/translation.md), [Traditional Chinese output](../input/traditional.md), [Sentences, abbreviations and correction](../input/sentence.md) |
| 中英文 (Chinese/English) | English mode, switch key, default mode, remembering per app, English candidates, punctuation, automatic spaces, floating status bar | [Chinese and English](../input/english-mode.md), [Punctuation, symbols and emoji](../input/punctuation.md) |
| 外观 (Appearance) | Theme, light/dark, horizontal/vertical layout, pinyin position, fonts and sizes, animation | [Appearance and themes](../personalize/appearance.md) |
| 快捷键 (Shortcuts) | Paging keys, highlight keys, prefix letters, modifiers for glosses, shortcuts for translation and AI suggestions | [Changing keys](../personalize/keys-settings.md) |
| 自定义短语 (Custom phrases) | Fixed phrases for input codes | [Custom phrases](../personalize/phrases.md) |
| 词库 (Dictionaries) | Turning bundled dictionaries on and off, importing your own | [Dictionaries](../personalize/dictionaries.md) |
| 云服务 (Cloud services) | Turning AI services on, the provider, each AI feature | [AI services](../cloud/index.md) |
| 统计 (Statistics) | Amount typed, speed, vocabulary progress, AI usage | [Statistics](../learning/statistics.md) |
| 生词本 (Vocabulary book) | Timeline and word list, export, clear | [Vocabulary book](../learning/vocabulary.md) |
| 实验功能 (Experimental features) | Qingjian Clipboard's switch, shortcut, direction, history limit, pausing, apps not to record and clearing history (macOS, Windows); download and turn on local sentence models: Zhiyi and Hanzhang Tongbian (macOS, Windows) | [Sentences, abbreviations and correction](../input/sentence.md#Experimental-features) |
| 高级 (Advanced) | Configuration file, logs, input log | [Privacy](../help/privacy.md), [Editing the configuration file](../advanced/config-file.md) |
| 关于 (About) | Version, update check, licenses, data sources, privacy notes | [Privacy](../help/privacy.md#Checking-for-updates) |

## The input method menu (macOS)

Click the 中 / 英 item in the menu bar to open it: the cloud suggestions switch, the fuzzy pinyin submenu, the symbol panel (opens at the cursor), Settings, legacy Settings, Open Log Folder and the version number. New versions are also announced here.

## The taskbar icon (Windows)

Right-click the 中 / 英 icon on the taskbar: switch to Chinese / English, full-width punctuation, show or hide the floating status bar, open the symbol panel, open Settings. New versions are also announced here.

## Legacy settings

macOS keeps the old settings window as a fallback: 旧版设置… (Legacy Settings…) in the input method menu. It changes the same settings as the new window.

Mengren for Windows no longer ships the legacy settings app after 0.5.0; the taskbar menu and Start menu entries are gone, and updating removes the app and its bundled runtime (about 70 MB). If the new settings window won't open, [edit the config file](../advanced/config-file.md) directly and please [report it](../help/feedback.md).

## Removed files (Windows)

Mengren for Windows only ships the data for learning English. After 0.5.0 these are no longer bundled and are removed on update: Japanese, Spanish, Vietnamese and German candidate translations; the Japanese level list; and the Vietnamese input word list. English translations, Chinese meanings for English candidates and the vocabulary book are unaffected.

To restore them, first choose the language in Settings (Candidates → Translation, or Vietnamese for the double-Shift input method), then install the **v0.5.0** Windows installer from the [Mengren GitHub releases page](https://github.com/longpeng1413/mengren-shurufa/releases) once, and update to the latest version as usual. The update keeps the files for a language that is selected in the config.
