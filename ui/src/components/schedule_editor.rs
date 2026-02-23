use dioxus::prelude::*;

use cream_common::storefront::WeeklySchedule;

/// Time options for the dropdowns: "00:00" through "23:30" in half-hour increments.
fn time_options() -> Vec<(u8, String)> {
    (0..48)
        .map(|slot| (slot, WeeklySchedule::format_slot_24h(slot)))
        .collect()
}

/// Editable weekly schedule with per-day time range rows.
#[component]
pub fn ScheduleEditor(
    schedule: WeeklySchedule,
    on_save: EventHandler<WeeklySchedule>,
) -> Element {
    // Internal state: Vec of 7 days, each a Vec of (start_slot, end_slot) ranges
    let mut ranges: Signal<Vec<Vec<(u8, u8)>>> = use_signal(|| {
        (0..7)
            .map(|day| schedule.get_ranges(day))
            .collect()
    });

    let options = time_options();

    let build_schedule = move || {
        let mut sched = WeeklySchedule::new();
        let current = ranges.read();
        for (day, day_ranges) in current.iter().enumerate() {
            for &(start, end) in day_ranges {
                sched.set_range(day as u8, start, end, true);
            }
        }
        sched
    };

    rsx! {
        div { class: "schedule-editor",
            for day in 0..7u8 {
                {
                    let day_ranges = ranges.read()[day as usize].clone();
                    let day_name = WeeklySchedule::day_name(day);
                    rsx! {
                        div { class: "schedule-day-row", key: "{day}",
                            span { class: "schedule-day-label", "{day_name}" }
                            div { class: "schedule-ranges",
                                if day_ranges.is_empty() {
                                    span { class: "schedule-closed-label", "(Closed)" }
                                } else {
                                    for (idx, (start, end)) in day_ranges.iter().enumerate() {
                                        {
                                            let start_val = *start;
                                            let end_val = *end;
                                            rsx! {
                                                div { class: "schedule-time-range", key: "{day}-{idx}",
                                                    select {
                                                        value: "{start_val}",
                                                        onchange: {
                                                            let idx = idx;
                                                            move |evt: Event<FormData>| {
                                                                let val: u8 = evt.value().parse().unwrap_or(0);
                                                                ranges.write()[day as usize][idx].0 = val;
                                                            }
                                                        },
                                                        {options.iter().map(|(slot, label)| {
                                                            let s = *slot;
                                                            rsx! { option { value: "{s}", "{label}" } }
                                                        })}
                                                    }
                                                    span { " to " }
                                                    select {
                                                        value: "{end_val}",
                                                        onchange: {
                                                            let idx = idx;
                                                            move |evt: Event<FormData>| {
                                                                let val: u8 = evt.value().parse().unwrap_or(48);
                                                                ranges.write()[day as usize][idx].1 = val;
                                                            }
                                                        },
                                                        {options.iter().map(|(slot, label)| {
                                                            let s = *slot;
                                                            rsx! { option { value: "{s}", "{label}" } }
                                                        })}
                                                        option { value: "48", "24:00" }
                                                    }
                                                    button {
                                                        class: "schedule-remove-btn",
                                                        onclick: {
                                                            let idx = idx;
                                                            move |_| {
                                                                ranges.write()[day as usize].remove(idx);
                                                            }
                                                        },
                                                        "×"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                button {
                                    class: "schedule-add-btn",
                                    onclick: move |_| {
                                        // Default new range: 8:00–17:00
                                        ranges.write()[day as usize].push((16, 34));
                                    },
                                    "+ Add"
                                }
                                if !day_ranges.is_empty() {
                                    button {
                                        class: "schedule-clear-btn",
                                        onclick: move |_| {
                                            ranges.write()[day as usize].clear();
                                        },
                                        "Clear"
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "schedule-actions",
                button {
                    onclick: move |_| {
                        // Copy Monday to Tue–Fri
                        let monday = ranges.read()[0].clone();
                        let mut w = ranges.write();
                        for day in 1..5 {
                            w[day] = monday.clone();
                        }
                    },
                    "Copy Mon → Weekdays"
                }
                button {
                    onclick: move |_| {
                        on_save.call(build_schedule());
                    },
                    "Save Schedule"
                }
            }
        }
    }
}

/// Compact read-only summary of the weekly schedule.
/// Groups consecutive days with identical hours.
#[component]
pub fn ScheduleSummary(schedule: WeeklySchedule) -> Element {
    let groups = group_days(&schedule);

    if groups.is_empty() {
        return rsx! {
            div { class: "schedule-summary",
                p { "No opening hours set" }
            }
        };
    }

    rsx! {
        div { class: "schedule-summary",
            for (label, hours) in groups.iter() {
                p { key: "{label}", "{label}: {hours}" }
            }
        }
    }
}

/// Group consecutive days with identical ranges for compact display.
fn group_days(schedule: &WeeklySchedule) -> Vec<(String, String)> {
    let day_ranges: Vec<Vec<(u8, u8)>> = (0..7).map(|d| schedule.get_ranges(d)).collect();
    let mut groups: Vec<(String, String)> = Vec::new();
    let mut i = 0;

    while i < 7 {
        let ranges = &day_ranges[i];
        let mut j = i + 1;
        while j < 7 && day_ranges[j] == *ranges {
            j += 1;
        }

        let label = if j - i == 1 {
            WeeklySchedule::day_name_short(i as u8).to_string()
        } else {
            format!(
                "{}–{}",
                WeeklySchedule::day_name_short(i as u8),
                WeeklySchedule::day_name_short((j - 1) as u8)
            )
        };

        let hours = if ranges.is_empty() {
            "Closed".to_string()
        } else {
            ranges
                .iter()
                .map(|(s, e)| {
                    format!(
                        "{} – {}",
                        WeeklySchedule::format_slot_12h(*s),
                        if *e == 48 {
                            "12:00 AM".to_string()
                        } else {
                            WeeklySchedule::format_slot_12h(*e)
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        groups.push((label, hours));
        i = j;
    }

    groups
}
