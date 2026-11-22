#!/usr/bin/env python3
"""生成划词翻译的英文词典卡片表 assets/wordcard/wordcard-en.tsv。

释义、考试标签与词形变化取自 ECDICT（MIT），美式音标由 CMUdict（BSD）的 ARPAbet 转成国际音标，
CMUdict 没有的词退回 ECDICT 自带的音标（不标「美」）。只收输入法英文词表里的词与带考试标签的词。

用法：
    python -I tools/corpus/wordcard.py ecdict.csv cmudict.dict \
        assets/lexicon/05_english/00_all_words.tsv assets/wordcard/wordcard-en.tsv
"""

import csv
import sys
from argparse import ArgumentParser
from pathlib import Path

# 一个词最多留几行释义、每行最多几个字
MAX_LINES = 6
MAX_CHARS = 100
# ECDICT 的考试标签，按显示顺序
TAGS = ["zk", "gk", "cet4", "cet6", "ky", "toefl", "ielts", "gre"]
# ECDICT exchange 里表示变形的代码：过去式、过去分词、现在分词、三单、比较级、最高级、复数
FORMS = set("pdi3rts")

# ARPAbet → 有道美式音标写法（元音长音带 ː，非重读 AH 为 ə，ER 按轻重写 ər / ɜːr）
VOWELS = {
    "AA": "ɑː", "AE": "æ", "AO": "ɔː", "AW": "aʊ", "AY": "aɪ", "EH": "e",
    "EY": "eɪ", "IH": "ɪ", "OW": "oʊ", "OY": "ɔɪ", "UH": "ʊ",
}
CONSONANTS = {
    "B": "b", "CH": "tʃ", "D": "d", "DH": "ð", "F": "f", "G": "ɡ", "HH": "h",
    "JH": "dʒ", "K": "k", "L": "l", "M": "m", "N": "n", "NG": "ŋ", "P": "p",
    "R": "r", "S": "s", "SH": "ʃ", "T": "t", "TH": "θ", "V": "v", "W": "w",
    "Y": "j", "Z": "z", "ZH": "ʒ",
}
# 英语词首允许的辅音连缀：重音符放在重读音节的这段声母之前（最大声母原则）
ONSETS = {
    tuple(onset.split())
    for onset in [
        "P L", "B L", "K L", "G L", "F L", "S L", "P R", "B R", "T R", "D R", "K R",
        "G R", "F R", "TH R", "SH R", "S W", "T W", "K W", "D W", "G W", "TH W",
        "S M", "S N", "S P", "S T", "S K", "S F", "S P L", "S P R", "S T R", "S K R",
        "S K W", "P Y", "B Y", "K Y", "G Y", "M Y", "F Y", "V Y", "HH Y", "N Y",
        "S K Y", "S P Y",
    ]
}


def vowel(phone: str) -> str | None:
    """元音的音标；不是元音回 None。"""
    base, stress = phone.rstrip("012"), phone[len(phone.rstrip("012")):]
    if not stress:
        return None
    if base == "AH":
        return "ə" if stress == "0" else "ʌ"
    if base == "ER":
        return "ər" if stress == "0" else "ɜːr"
    if base == "IY":
        return "i" if stress == "0" else "iː"
    if base == "UW":
        return "u" if stress == "0" else "uː"
    return VOWELS[base]


def american(phones: list[str]) -> str:
    """ARPAbet 音素串转美式音标；单音节词不标重音。"""
    nuclei = [i for i, phone in enumerate(phones) if phone[-1].isdigit()]
    marks: dict[int, str] = {}
    if len(nuclei) > 1:
        previous = -1
        for index in nuclei:
            stress = phones[index][-1]
            if stress in "12":
                cluster = phones[previous + 1:index]
                start = index
                for size in range(len(cluster), 0, -1):
                    tail = tuple(cluster[-size:])
                    if size == 1 and tail[0] != "NG" or tail in ONSETS:
                        start = index - size
                        break
                if previous < 0:
                    start = 0
                marks[start] = "ˈ" if stress == "1" else "ˌ"
            previous = index
    out = []
    for i, phone in enumerate(phones):
        out.append(marks.get(i, ""))
        out.append(vowel(phone) or CONSONANTS[phone])
    return "".join(out)


def load_cmudict(path: Path) -> dict[str, str]:
    """每个词取第一种读音；带 (2) 的异读跳过。"""
    result: dict[str, str] = {}
    with path.open(encoding="utf-8") as source:
        for line in source:
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            word, *phones = line.split()
            if "(" in word or word in result:
                continue
            try:
                result[word] = american(phones)
            except KeyError:
                continue
    return result


def ecdict_phonetic(text: str) -> str:
    """ECDICT 自带音标的写法改成国际音标符号。"""
    return text.replace("'", "ˈ").replace(",", "ˌ").replace(":", "ː").strip()


def definitions(text: str) -> list[str]:
    """中文释义行：去掉「[网络]」这类网络释义，截短。"""
    lines = []
    for line in text.replace("\\n", "\n").splitlines():
        line = " ".join(line.replace("\t", " ").split())
        if not line or line.startswith("[网络]"):
            continue
        if line.startswith("a. "):
            line = "adj. " + line[3:]
        if len(line) > MAX_CHARS:
            line = line[: MAX_CHARS - 1] + "…"
        lines.append(line)
        if len(lines) == MAX_LINES:
            break
    return lines


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("ecdict", type=Path)
    parser.add_argument("cmudict", type=Path)
    parser.add_argument("words", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    wanted: set[str] = set()
    with args.words.open(encoding="utf-8") as source:
        next(source)
        for line in source:
            wanted.add(line.split("\t", 1)[0].strip().lower())

    csv.field_size_limit(sys.maxsize)
    rows: dict[str, dict[str, str]] = {}
    lemma_of: dict[str, str] = {}
    with args.ecdict.open(encoding="utf-8", newline="") as source:
        for row in csv.DictReader(source):
            word = row["word"].strip()
            if not word or "\t" in word:
                continue
            rows.setdefault(word, row)
            for part in row["exchange"].split("/"):
                code, _, form = part.partition(":")
                form = form.strip()
                if code == "0" and form and form != word:
                    lemma_of.setdefault(word.lower(), form)
                elif code in FORMS and form and form.lower() != word.lower():
                    lemma_of.setdefault(form.lower(), word)

    cmu = load_cmudict(args.cmudict)
    lines = []
    for word, row in rows.items():
        key = word.lower()
        tags = [tag for tag in TAGS if tag in row["tag"].split()]
        if key not in wanted and not tags:
            continue
        defs = definitions(row["translation"])
        lemma = lemma_of.get(key, "")
        if lemma.lower() == key:
            lemma = ""
        if not defs and not lemma:
            continue
        if key in cmu:
            phonetic, mark = cmu[key], "us"
        else:
            phonetic, mark = ecdict_phonetic(row["phonetic"]), ""
        lines.append("\t".join([word, lemma, phonetic, mark, " ".join(tags), "\\n".join(defs)]))

    lines.sort()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="\n") as output:
        output.write(
            "# 由 tools/corpus/wordcard.py 从 ECDICT（MIT）与 CMUdict（BSD）生成，见同目录 README。\n"
            "# 词\t原形\t音标\t音标来源（us 为 CMUdict 美式）\t考试标签\t释义（\\n 分行）\n"
        )
        output.write("\n".join(lines) + "\n")
    print(f"{len(lines)} entries, {sum(1 for l in lines if l.split(chr(9))[3] == 'us')} with US phonetics")


if __name__ == "__main__":
    main()
