//! 查到的一张英文词典卡片。

/// 划词翻译弹出的词典卡片内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordCard {
    /// 卡片标题：表里的写法（选中 `Running` 时是 `running`）。
    pub word: String,

    /// 变形词的原形（`running` → `run`）。
    pub lemma: Option<String>,

    /// 音标，不带斜杠。
    pub phonetic: Option<String>,

    /// 音标是不是美式（CMUdict 转来的）；否则是 ECDICT 自带的，不标英美。
    pub american: bool,

    /// 考试标签代码（`cet4`、`ky`……），显示名由界面层翻。
    pub tags: Vec<String>,

    /// 中文释义，一行一个词性（`n. 跑, 赛跑`）。
    pub definitions: Vec<String>,
}
