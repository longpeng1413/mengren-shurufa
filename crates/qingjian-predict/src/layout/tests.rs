use super::Template;

#[test]
fn keeps_blank_lines_markers_and_tags_out_of_the_text() {
    let text = "1. Be clear and specific\r\n\r\n  • State your task.\nBad prompt: <prompt> \"Help me.\" </prompt>\n";
    let template = Template::parse(text);
    assert_eq!(
        template.texts(),
        [
            "Be clear and specific",
            "State your task.",
            "Bad prompt:",
            "\"Help me.\""
        ]
    );
    let filled = template.fill(&["要清晰具体", "说明任务。", "差的提示：", "“帮帮我。”"]);
    assert_eq!(
        filled.as_deref(),
        Some("1. 要清晰具体\n\n  • 说明任务。\n差的提示： <prompt> “帮帮我。” </prompt>\n")
    );
}

#[test]
fn ordinary_text_is_not_mistaken_for_markup_or_lists() {
    let template = Template::parse("-5 degrees, 3.14 and a < b > c");
    assert_eq!(template.texts(), ["-5 degrees, 3.14 and a < b > c"]);
    let template = Template::parse("3、注意事项");
    assert_eq!(template.texts(), ["注意事项"]);
}

#[test]
fn symbols_and_numbers_stay_untranslated() {
    let template = Template::parse("---\n2026-11-22\n\nHello");
    assert_eq!(template.texts(), ["Hello"]);
    assert_eq!(
        template.fill(&["你好"]).as_deref(),
        Some("---\n2026-11-22\n\n你好")
    );
}

#[test]
fn a_wrong_number_of_translations_is_rejected() {
    let template = Template::parse("One\nTwo");
    assert_eq!(template.fill(&["一二"]), None);
    assert_eq!(template.fill(&["一", "二"]).as_deref(), Some("一\n二"));
}
