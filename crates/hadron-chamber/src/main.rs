//! The chamber: Hadron's viewer. Without `--features gui` this is a headless
//! smoke binary that projects a field file and prints row counts (proves the
//! model links without GPUI). With `gui`, it launches the GPUI window (Task 3+).

mod model;

#[cfg(feature = "gui")]
mod app;

fn main() {
    let path = std::env::args().nth(1);

    #[cfg(feature = "gui")]
    {
        app::run(path);
    }

    #[cfg(not(feature = "gui"))]
    {
        run_headless(path);
    }
}

#[cfg(not(feature = "gui"))]
fn run_headless(path: Option<String>) {
    match path {
        Some(p) => {
            let events =
                hadron_lattice::io::read_events(std::path::Path::new(&p)).unwrap_or_default();
            let view = model::project(&events);
            println!(
                "chamber: {} chat row(s), {} quark(s) on roster",
                view.messages.len(),
                view.roster.len()
            );
        }
        None => eprintln!("usage: hadron-chamber <field.jsonl>"),
    }
}
