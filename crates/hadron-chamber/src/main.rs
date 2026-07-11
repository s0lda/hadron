//! The chamber: Hadron's viewer. Without `--features gui` this is a headless
//! smoke binary that projects a field file and prints row counts (proves the
//! model links without GPUI). With `gui`, it launches the GPUI window.

mod model;

// Layout persistence is used by the window; keep it compiled (and unit-tested)
// in the default build, but don't warn when the headless binary ignores it.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod config;

#[cfg(feature = "gui")]
mod app;
#[cfg(feature = "gui")]
mod theme;

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
