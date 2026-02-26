use dioxus::prelude::*;

use super::accordion_md::AccordionMarkdown;

const FAQ_MD: &str = include_str!("../../../docs/faq.md");

#[component]
pub fn FaqView() -> Element {
    rsx! {
        AccordionMarkdown { source: FAQ_MD }
    }
}
