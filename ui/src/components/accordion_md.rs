use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser, html};

#[derive(Clone, PartialEq)]
struct Section {
    title: String,
    body_html: String,
}

fn parse_accordion(source: &str) -> (String, Vec<Section>) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    // Split on ## headers. Everything before the first ## is the preamble.
    let mut preamble_md = String::new();
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();

    for line in source.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            // Flush previous section
            if let Some(prev_title) = current_title.take() {
                let mut html_out = String::new();
                html::push_html(&mut html_out, Parser::new_ext(&current_body, opts));
                sections.push(Section {
                    title: prev_title,
                    body_html: html_out,
                });
                current_body.clear();
            }
            current_title = Some(title.to_string());
        } else if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        } else {
            preamble_md.push_str(line);
            preamble_md.push('\n');
        }
    }

    // Flush last section
    if let Some(title) = current_title {
        let mut html_out = String::new();
        html::push_html(&mut html_out, Parser::new_ext(&current_body, opts));
        sections.push(Section {
            title,
            body_html: html_out,
        });
    }

    let mut preamble_html = String::new();
    html::push_html(&mut preamble_html, Parser::new_ext(&preamble_md, opts));

    (preamble_html, sections)
}

#[component]
pub fn AccordionMarkdown(source: &'static str) -> Element {
    let parsed = use_hook(|| parse_accordion(source));
    let mut expanded = use_signal::<Option<usize>>(|| None);

    let (preamble_html, sections) = &parsed;

    rsx! {
        div { class: "iaq-view",
            div {
                class: "iaq-content",
                dangerous_inner_html: "{preamble_html}"
            }
            for (i, section) in sections.iter().enumerate() {
                div { class: "accordion-section",
                    div {
                        class: "accordion-header",
                        onclick: move |_| {
                            let current = expanded();
                            if current == Some(i) {
                                expanded.set(None);
                            } else {
                                expanded.set(Some(i));
                            }
                        },
                        span { class: "accordion-chevron",
                            if expanded() == Some(i) { "▾" } else { "▸" }
                        }
                        "{section.title}"
                    }
                    if expanded() == Some(i) {
                        div {
                            class: "accordion-body iaq-content",
                            dangerous_inner_html: "{section.body_html}"
                        }
                    }
                }
            }
        }
    }
}
