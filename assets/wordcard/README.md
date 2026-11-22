# 英文词典卡片表

`wordcard-en.tsv`：Windows 划词翻译选中单个英文词时弹出的词典卡片（美式音标、中文释义、考试标签、原形），
由 `qingjian-translate::WordCards` 读，随 Windows 安装包装到 `assets\wordcard\`。

格式：`词\t原形\t音标\t音标来源\t考试标签\t释义`，`#` 开头为注释，UTF-8 无 BOM、LF 换行、按码点排序。

- 原形：变形词指向原形（`running` → `run`），没有留空；释义为空的行只做跳转。
- 音标来源：`us` 表示由 CMUdict 转成的美式音标；空表示 CMUdict 没收这个词，用的是 ECDICT 自带音标。
- 考试标签：`zk gk cet4 cet6 ky toefl ielts gre`（中考、高考、四级、六级、考研、托福、雅思、GRE），空格分隔。
- 释义：一行一个词性，行间写成两个字符 `\n`；去掉了 ECDICT 的「[网络]」释义，最多 6 行。

| 来源 | 用途 | 许可 |
| --- | --- | --- |
| ECDICT，Linwei（skywind3000），<https://github.com/skywind3000/ECDICT> | 中文释义、考试标签、词形变化、后备音标 | MIT，见 `LICENSE-ECDICT.txt` |
| CMUdict，Carnegie Mellon University，<https://github.com/cmusphinx/cmudict> | 美式读音（ARPAbet 转国际音标） | BSD 2-Clause 式，见 `LICENSE-CMUdict.txt` |

两份许可证随安装包一起装。收词范围是 `assets/lexicon/05_english/00_all_words.tsv` 里的词，加上 ECDICT 中带考试标签的词。

重新生成（原始文件不进 git，下载到任意目录）：

```bash
curl -LO https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv
curl -LO https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict
python -I tools/corpus/wordcard.py ecdict.csv cmudict.dict \
    assets/lexicon/05_english/00_all_words.tsv assets/wordcard/wordcard-en.tsv
```

ARPAbet 转音标按有道的美式写法：长元音带 `ː`，非重读 `AH` 写 `ə`、重读写 `ʌ`，`ER` 按轻重写 `ər` / `ɜːr`，
重音符按最大声母原则放在重读音节的声母前，单音节词不标重音。
