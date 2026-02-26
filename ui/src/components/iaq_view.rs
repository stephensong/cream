use dioxus::prelude::*;

use super::accordion_md::AccordionMarkdown;

const IAQ_MD: &str = include_str!("../../../docs/IAQ.md");

#[component]
pub fn IaqView() -> Element {
    rsx! {
        AccordionMarkdown { source: IAQ_MD }
    }
}
