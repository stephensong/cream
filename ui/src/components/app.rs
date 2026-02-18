use dioxus::prelude::*;

use super::directory_view::DirectoryView;
use super::supplier_dashboard::SupplierDashboard;
use super::wallet_view::WalletView;

#[derive(Clone, PartialEq)]
enum View {
    Directory,
    SupplierDashboard,
    Wallet,
}

#[component]
pub fn App() -> Element {
    let mut current_view = use_signal(|| View::Directory);

    rsx! {
        div { class: "cream-app",
            header { class: "app-header",
                h1 { "CREAM" }
                p { "CURD Retail Exchange And Marketplace" }
                nav {
                    button {
                        onclick: move |_| current_view.set(View::Directory),
                        "Browse Suppliers"
                    }
                    button {
                        onclick: move |_| current_view.set(View::SupplierDashboard),
                        "Supplier Dashboard"
                    }
                    button {
                        onclick: move |_| current_view.set(View::Wallet),
                        "Wallet"
                    }
                }
            }
            main {
                match *current_view.read() {
                    View::Directory => rsx! { DirectoryView {} },
                    View::SupplierDashboard => rsx! { SupplierDashboard {} },
                    View::Wallet => rsx! { WalletView {} },
                }
            }
        }
    }
}
