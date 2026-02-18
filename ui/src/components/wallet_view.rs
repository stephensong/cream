use dioxus::prelude::*;

#[component]
pub fn WalletView() -> Element {
    let mut balance = use_signal(|| 10_000u64);

    rsx! {
        div { class: "wallet-view",
            h2 { "CURD Wallet" }
            div { class: "balance-display",
                h3 { "Balance: {balance} CURD" }
            }
            div { class: "wallet-actions",
                button {
                    onclick: move |_| balance.set(balance() + 1000),
                    "Faucet (+1000 CURD)"
                }
            }
            p { class: "wallet-note",
                "This is a mock wallet for demonstration. Fedimint integration coming later."
            }
        }
    }
}
