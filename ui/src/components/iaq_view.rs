use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser, html};

const IAQ_MD: &str = include_str!("../../../docs/IAQ.md");

#[component]
pub fn IaqView() -> Element {
    let html_content = use_memo(move || {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        let parser = Parser::new_ext(IAQ_MD, opts);
        let mut html_output = String::new();
        html::push_html(&mut html_output, parser);
        html_output
    });

    rsx! {
        div { class: "iaq-view",
            div {
                class: "iaq-content",
                dangerous_inner_html: "{html_content}"
            }
        }
    }
}
